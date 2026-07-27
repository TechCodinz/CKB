//! OpenTelemetry OTLP Native Receiver
//! Accepts OTLP/HTTP JSON span payloads and maps them to CKB graph NodeIds

use serde::{Deserialize, Serialize};
use crate::types::{NodeId, RuntimeMetrics};
use std::collections::HashMap;

/// An individual OTLP span as received from Jaeger/Datadog/Honeycomb
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OtlpSpan {
    pub trace_id: String,
    pub span_id: String,
    pub name: String,
    pub parent_span_id: Option<String>,
    pub start_time_unix_nano: u64,
    pub end_time_unix_nano: u64,
    pub status: Option<OtlpSpanStatus>,
    pub attributes: HashMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OtlpSpanStatus {
    pub code: u32, // 0=OK, 1=ERROR
    pub message: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OtlpIngestReport {
    pub spans_ingested: usize,
    pub nodes_updated: usize,
    pub error_spans: usize,
    pub hotpath_nodes: Vec<String>,
}

pub struct OtlpReceiver;

impl OtlpReceiver {
    /// Parse raw OTLP/HTTP JSON payload and extract RuntimeMetrics per span name
    pub fn ingest_spans(raw_payload: &str) -> anyhow::Result<HashMap<NodeId, RuntimeMetrics>> {
        let spans: Vec<OtlpSpan> = match serde_json::from_str(raw_payload) {
            Ok(v) => v,
            Err(_) => {
                // Try nested OTLP format: { "resourceSpans": [ { "scopeSpans": [ { "spans": [...] } ] } ] }
                let val: serde_json::Value = serde_json::from_str(raw_payload)?;
                let spans_arr = val
                    .get("resourceSpans")
                    .and_then(|rs| rs.as_array())
                    .and_then(|rs| rs.first())
                    .and_then(|r| r.get("scopeSpans"))
                    .and_then(|ss| ss.as_array())
                    .and_then(|ss| ss.first())
                    .and_then(|s| s.get("spans"))
                    .and_then(|sp| sp.as_array())
                    .cloned()
                    .unwrap_or_default();

                serde_json::from_value(serde_json::Value::Array(spans_arr))
                    .unwrap_or_default()
            }
        };

        let mut metrics_map: HashMap<String, (u64, u64, u32)> = HashMap::new(); // name -> (count, total_nanos, errors)

        for span in &spans {
            let duration_ns = span.end_time_unix_nano.saturating_sub(span.start_time_unix_nano);
            let is_error = span.status.as_ref().map(|s| s.code == 1).unwrap_or(false);

            let entry = metrics_map.entry(span.name.clone()).or_insert((0, 0, 0));
            entry.0 += 1;
            entry.1 += duration_ns;
            if is_error { entry.2 += 1; }
        }

        let mut result: HashMap<NodeId, RuntimeMetrics> = HashMap::new();

        for (name, (count, total_ns, errors)) in metrics_map {
            let avg_latency_ms = if count > 0 {
                (total_ns / count) as f32 / 1_000_000.0
            } else {
                0.0
            };
            let error_rate = if count > 0 { errors as f32 / count as f32 } else { 0.0 };

            result.insert(NodeId(name), RuntimeMetrics {
                execution_count: count,
                avg_latency_ms,
                error_rate,
                is_hotpath: count > 500,
            });
        }

        Ok(result)
    }

    /// Produce a summary report from an OTLP ingest operation
    pub fn summarize(metrics: &HashMap<NodeId, RuntimeMetrics>) -> OtlpIngestReport {
        let hotpaths: Vec<String> = metrics
            .iter()
            .filter(|(_, m)| m.is_hotpath)
            .map(|(id, _)| id.0.clone())
            .collect();

        let error_spans = metrics
            .values()
            .filter(|m| m.error_rate > 0.0)
            .count();

        OtlpIngestReport {
            spans_ingested: metrics.values().map(|m| m.execution_count as usize).sum(),
            nodes_updated: metrics.len(),
            error_spans,
            hotpath_nodes: hotpaths,
        }
    }
}
