//! OpenTelemetry OTLP Native Receiver
//! Accepts OTLP/HTTP JSON span payloads and maps them to CKB graph NodeIds.
//!
//! Architecture Intelligence V2 rule: runtime observations must preserve
//! provenance and source identity where available. A span name is only used as
//! a final fallback because names alone are frequently ambiguous across files.

use serde::{Deserialize, Serialize};
use crate::types::{NodeId, RuntimeMetrics};
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OtlpSpan {
    pub trace_id: String,
    pub span_id: String,
    pub name: String,
    pub parent_span_id: Option<String>,
    pub start_time_unix_nano: u64,
    pub end_time_unix_nano: u64,
    pub status: Option<OtlpSpanStatus>,
    #[serde(default)]
    pub attributes: HashMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OtlpSpanStatus {
    pub code: u32,
    pub message: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OtlpIngestReport {
    pub spans_ingested: usize,
    pub nodes_updated: usize,
    pub error_spans: usize,
    pub hotpath_nodes: Vec<String>,
}

#[derive(Debug, Clone)]
struct NormalizedSpan {
    name: String,
    start_ns: u64,
    end_ns: u64,
    error: bool,
    attributes: HashMap<String, String>,
}

pub struct OtlpReceiver;

impl OtlpReceiver {
    fn scalar_to_string(value: &serde_json::Value) -> Option<String> {
        if let Some(s) = value.as_str() { return Some(s.to_string()); }
        if let Some(v) = value.get("stringValue").and_then(|v| v.as_str()) { return Some(v.to_string()); }
        if let Some(v) = value.get("intValue") {
            if let Some(s) = v.as_str() { return Some(s.to_string()); }
            if let Some(n) = v.as_i64() { return Some(n.to_string()); }
            if let Some(n) = v.as_u64() { return Some(n.to_string()); }
        }
        if let Some(v) = value.get("doubleValue").and_then(|v| v.as_f64()) { return Some(v.to_string()); }
        if let Some(v) = value.get("boolValue").and_then(|v| v.as_bool()) { return Some(v.to_string()); }
        if let Some(n) = value.as_i64() { return Some(n.to_string()); }
        if let Some(n) = value.as_u64() { return Some(n.to_string()); }
        if let Some(n) = value.as_f64() { return Some(n.to_string()); }
        if let Some(b) = value.as_bool() { return Some(b.to_string()); }
        None
    }

    fn parse_attributes(value: Option<&serde_json::Value>) -> HashMap<String, String> {
        let mut out = HashMap::new();
        let Some(value) = value else { return out; };

        if let Some(obj) = value.as_object() {
            for (key, val) in obj {
                if let Some(s) = Self::scalar_to_string(val) {
                    out.insert(key.clone(), s);
                }
            }
            return out;
        }

        if let Some(items) = value.as_array() {
            for item in items {
                let Some(key) = item.get("key").and_then(|v| v.as_str()) else { continue; };
                let val = item.get("value").unwrap_or(&serde_json::Value::Null);
                if let Some(s) = Self::scalar_to_string(val) {
                    out.insert(key.to_string(), s);
                }
            }
        }
        out
    }

    fn u64_value(value: Option<&serde_json::Value>) -> u64 {
        value.and_then(|v| v.as_u64())
            .or_else(|| value.and_then(|v| v.as_str()).and_then(|s| s.parse::<u64>().ok()))
            .unwrap_or(0)
    }

    fn status_is_error(status: Option<&serde_json::Value>) -> bool {
        let Some(status) = status else { return false; };
        if let Some(code) = status.get("code") {
            // OTLP StatusCode enum: UNSET=0, OK=1, ERROR=2.
            // Numeric OK must never be counted as an error.
            if let Some(n) = code.as_u64() {
                return n == 2;
            }
            if let Some(s) = code.as_str() {
                if let Ok(n) = s.parse::<u64>() {
                    return n == 2;
                }
                return matches!(s.to_ascii_uppercase().as_str(), "ERROR" | "STATUS_CODE_ERROR");
            }
        }
        false
    }

    fn normalize_span(value: &serde_json::Value) -> Option<NormalizedSpan> {
        let name = value.get("name")?.as_str()?.to_string();
        let start_ns = Self::u64_value(value.get("startTimeUnixNano").or_else(|| value.get("start_time_unix_nano")));
        let end_ns = Self::u64_value(value.get("endTimeUnixNano").or_else(|| value.get("end_time_unix_nano")));
        let attributes = Self::parse_attributes(value.get("attributes"));
        let error = Self::status_is_error(value.get("status"));
        Some(NormalizedSpan { name, start_ns, end_ns, error, attributes })
    }

    fn collect_span_values(root: &serde_json::Value) -> Vec<serde_json::Value> {
        if let Some(arr) = root.as_array() {
            return arr.clone();
        }

        let mut spans = Vec::new();
        if let Some(resources) = root.get("resourceSpans").and_then(|v| v.as_array()) {
            for resource in resources {
                let scopes = resource.get("scopeSpans")
                    .or_else(|| resource.get("instrumentationLibrarySpans"))
                    .and_then(|v| v.as_array());
                if let Some(scopes) = scopes {
                    for scope in scopes {
                        if let Some(items) = scope.get("spans").and_then(|v| v.as_array()) {
                            spans.extend(items.iter().cloned());
                        }
                    }
                }
            }
        }
        spans
    }

    fn canonical_node_id(span: &NormalizedSpan) -> NodeId {
        let file = span.attributes.get("code.file.path")
            .or_else(|| span.attributes.get("code.filepath"))
            .or_else(|| span.attributes.get("code.file.name"));
        let function = span.attributes.get("code.function.name")
            .or_else(|| span.attributes.get("code.function"))
            .or_else(|| span.attributes.get("function.name"));
        let namespace = span.attributes.get("code.namespace")
            .or_else(|| span.attributes.get("service.name"));

        match (file, function, namespace) {
            (Some(file), Some(function), _) => NodeId(format!("{}::{}", file.replace('\\', "/"), function)),
            (Some(file), None, _) => NodeId(format!("{}::file", file.replace('\\', "/"))),
            (None, Some(function), Some(ns)) => NodeId(format!("{}::{}", ns, function)),
            (None, Some(function), None) => NodeId(function.clone()),
            (None, None, Some(ns)) => NodeId(format!("{}::{}", ns, span.name)),
            _ => NodeId(span.name.clone()),
        }
    }

    pub fn ingest_spans(raw_payload: &str) -> anyhow::Result<HashMap<NodeId, RuntimeMetrics>> {
        let root: serde_json::Value = serde_json::from_str(raw_payload)?;
        let raw_spans = Self::collect_span_values(&root);
        let spans: Vec<NormalizedSpan> = raw_spans.iter().filter_map(Self::normalize_span).collect();

        let mut metrics_map: HashMap<NodeId, (u64, u128, u64)> = HashMap::new();
        for span in &spans {
            let duration_ns = span.end_ns.saturating_sub(span.start_ns) as u128;
            let id = Self::canonical_node_id(span);
            let entry = metrics_map.entry(id).or_insert((0, 0, 0));
            entry.0 += 1;
            entry.1 += duration_ns;
            if span.error { entry.2 += 1; }
        }

        let mut result = HashMap::new();
        for (id, (count, total_ns, errors)) in metrics_map {
            let avg_latency_ms = if count > 0 {
                (total_ns as f64 / count as f64 / 1_000_000.0) as f32
            } else { 0.0 };
            let error_rate = if count > 0 { errors as f32 / count as f32 } else { 0.0 };
            result.insert(id, RuntimeMetrics {
                execution_count: count,
                avg_latency_ms,
                error_rate,
                is_hotpath: count > 500,
            });
        }

        Ok(result)
    }

    pub fn summarize(metrics: &HashMap<NodeId, RuntimeMetrics>) -> OtlpIngestReport {
        let hotpaths = metrics.iter()
            .filter(|(_, m)| m.is_hotpath)
            .map(|(id, _)| id.0.clone())
            .collect();
        let error_spans = metrics.values().filter(|m| m.error_rate > 0.0).count();

        OtlpIngestReport {
            spans_ingested: metrics.values().map(|m| m.execution_count as usize).sum(),
            nodes_updated: metrics.len(),
            error_spans,
            hotpath_nodes: hotpaths,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ingests_all_standard_otlp_resources_and_maps_source_identity() {
        let payload = r#"{
          "resourceSpans": [{"scopeSpans": [{"spans": [
            {"name":"login","startTimeUnixNano":"1000000","endTimeUnixNano":"3000000","attributes":[
              {"key":"code.file.path","value":{"stringValue":"src/auth.ts"}},
              {"key":"code.function.name","value":{"stringValue":"login"}}
            ],"status":{"code":"STATUS_CODE_OK"}},
            {"name":"login","startTimeUnixNano":"4000000","endTimeUnixNano":"9000000","attributes":[
              {"key":"code.file.path","value":{"stringValue":"src/auth.ts"}},
              {"key":"code.function.name","value":{"stringValue":"login"}}
            ],"status":{"code":"STATUS_CODE_ERROR"}}
          ]}]}]
        }"#;

        let metrics = OtlpReceiver::ingest_spans(payload).unwrap();
        let m = metrics.get(&NodeId("src/auth.ts::login".to_string())).unwrap();
        assert_eq!(m.execution_count, 2);
        assert!((m.avg_latency_ms - 3.5).abs() < 0.001);
        assert!((m.error_rate - 0.5).abs() < 0.001);
    }

    #[test]
    fn numeric_otlp_ok_is_not_an_error_but_numeric_error_is() {
        let payload = r#"[
          {"name":"ok","startTimeUnixNano":"0","endTimeUnixNano":"1000000","attributes":{},"status":{"code":1}},
          {"name":"bad","startTimeUnixNano":"0","endTimeUnixNano":"1000000","attributes":{},"status":{"code":2}}
        ]"#;
        let metrics = OtlpReceiver::ingest_spans(payload).unwrap();
        assert_eq!(metrics.get(&NodeId("ok".into())).unwrap().error_rate, 0.0);
        assert_eq!(metrics.get(&NodeId("bad".into())).unwrap().error_rate, 1.0);
    }
}
