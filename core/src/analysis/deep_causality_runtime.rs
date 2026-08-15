//! Observed runtime-resource evidence ingestion for V13.1.
//!
//! This module accepts measurements produced by profilers/APM/telemetry
//! adapters. It never synthesizes CPU or memory usage from static structure.
//! Numeric attributes are observation-count weighted and every observation is
//! represented by a RUNTIME `Trace -> Observes -> entity` fact.

use crate::analysis::{
    CausalEntity, CausalEntityKind, CausalEvidenceClass, CausalFact,
    CausalRelationKind, DeepCausalityEngine,
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuntimeResourceObservation {
    pub entity_id: String,
    #[serde(default)] pub repository: Option<String>,
    #[serde(default)] pub cpu_ms: Option<f64>,
    #[serde(default)] pub memory_bytes: Option<u64>,
    #[serde(default)] pub latency_ms: Option<f64>,
    #[serde(default)] pub error_rate: Option<f64>,
    #[serde(default = "one_sample")] pub sample_count: u64,
    #[serde(default)] pub timestamp_ms: Option<i64>,
    #[serde(default)] pub trace_id: Option<String>,
    #[serde(default)] pub metadata: BTreeMap<String, String>,
}
fn one_sample() -> u64 { 1 }

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RuntimeResourceIngestReport {
    pub observations_ingested: usize,
    pub entities_updated: usize,
    pub total_samples: u64,
}

pub fn ingest_runtime_resource_observations(
    engine: &mut DeepCausalityEngine,
    observations: &[RuntimeResourceObservation],
) -> RuntimeResourceIngestReport {
    let mut updated = std::collections::HashSet::new();
    let mut total_samples = 0u64;

    for (index, observation) in observations.iter().enumerate() {
        let incoming_count = observation.sample_count.max(1);
        total_samples = total_samples.saturating_add(incoming_count);
        let existing = engine.entities().find(|e| e.id == observation.entity_id).cloned();
        let mut entity = existing.unwrap_or_else(|| CausalEntity {
            id: observation.entity_id.clone(),
            kind: CausalEntityKind::RuntimeResource,
            name: observation.entity_id.clone(),
            repository: observation.repository.clone(),
            path: None,
            attributes: BTreeMap::new(),
        });
        if entity.repository.is_none() { entity.repository = observation.repository.clone(); }
        let previous_count = entity.attributes.get("runtime.sample_count").and_then(|v| v.parse::<u64>().ok()).unwrap_or(0);
        let new_count = previous_count.saturating_add(incoming_count);

        merge_f64(&mut entity.attributes, "runtime.cpu_ms", observation.cpu_ms, previous_count, incoming_count);
        merge_u64(&mut entity.attributes, "runtime.memory_bytes", observation.memory_bytes, previous_count, incoming_count);
        merge_f64(&mut entity.attributes, "runtime.latency_ms", observation.latency_ms, previous_count, incoming_count);
        merge_f64(&mut entity.attributes, "runtime.error_rate", observation.error_rate.map(|v| v.clamp(0.0, 1.0)), previous_count, incoming_count);
        entity.attributes.insert("runtime.sample_count".into(), new_count.to_string());
        if let Some(ts) = observation.timestamp_ms { entity.attributes.insert("runtime.last_timestamp_ms".into(), ts.to_string()); }
        engine.upsert_entity(entity.clone());
        updated.insert(entity.id.clone());

        let trace_id = observation.trace_id.clone().unwrap_or_else(|| format!(
            "runtime:observation:{}:{}:{}",
            observation.entity_id,
            observation.timestamp_ms.unwrap_or(0),
            index
        ));
        if !engine.entities().any(|e| e.id == trace_id) {
            engine.upsert_entity(CausalEntity {
                id: trace_id.clone(), kind: CausalEntityKind::Trace, name: trace_id.clone(),
                repository: observation.repository.clone(), path: None,
                attributes: BTreeMap::new(),
            });
        }
        let mut metadata = observation.metadata.clone();
        metadata.insert("runtime.sample_count".into(), incoming_count.to_string());
        if let Some(v)=observation.cpu_ms { metadata.insert("runtime.cpu_ms".into(), v.to_string()); }
        if let Some(v)=observation.memory_bytes { metadata.insert("runtime.memory_bytes".into(), v.to_string()); }
        if let Some(v)=observation.latency_ms { metadata.insert("runtime.latency_ms".into(), v.to_string()); }
        if let Some(v)=observation.error_rate { metadata.insert("runtime.error_rate".into(), v.to_string()); }
        let duplicate = engine.facts().iter().any(|f| f.from == trace_id && f.to == entity.id && f.relation == CausalRelationKind::Observes && f.evidence == CausalEvidenceClass::Runtime);
        if !duplicate {
            let _ = engine.add_fact(CausalFact {
                from: trace_id,
                to: entity.id,
                relation: CausalRelationKind::Observes,
                evidence: CausalEvidenceClass::Runtime,
                confidence: 1.0,
                condition: None,
                timestamp_ms: observation.timestamp_ms,
                metadata,
            });
        }
    }

    RuntimeResourceIngestReport {
        observations_ingested: observations.len(),
        entities_updated: updated.len(),
        total_samples,
    }
}

fn merge_f64(attributes: &mut BTreeMap<String,String>, key: &str, incoming: Option<f64>, previous_count: u64, incoming_count: u64) {
    let Some(incoming) = incoming.filter(|v| v.is_finite()) else { return; };
    let previous = attributes.get(key).and_then(|v| v.parse::<f64>().ok());
    let value = match previous {
        Some(previous) if previous_count > 0 => ((previous * previous_count as f64) + (incoming * incoming_count as f64)) / (previous_count + incoming_count) as f64,
        _ => incoming,
    };
    attributes.insert(key.into(), value.to_string());
}
fn merge_u64(attributes: &mut BTreeMap<String,String>, key: &str, incoming: Option<u64>, previous_count: u64, incoming_count: u64) {
    let Some(incoming) = incoming else { return; };
    let previous = attributes.get(key).and_then(|v| v.parse::<u64>().ok());
    let value = match previous {
        Some(previous) if previous_count > 0 => {
            let weighted = previous as u128 * previous_count as u128 + incoming as u128 * incoming_count as u128;
            (weighted / (previous_count + incoming_count) as u128).min(u64::MAX as u128) as u64
        },
        _ => incoming,
    };
    attributes.insert(key.into(), value.to_string());
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resource_metrics_are_weighted_by_sample_count() {
        let mut engine=DeepCausalityEngine::new();
        ingest_runtime_resource_observations(&mut engine,&[
            RuntimeResourceObservation{entity_id:"svc".into(),repository:Some("r".into()),cpu_ms:Some(10.0),memory_bytes:Some(100),latency_ms:Some(20.0),error_rate:Some(0.1),sample_count:10,timestamp_ms:Some(1),trace_id:Some("t1".into()),metadata:BTreeMap::new()},
            RuntimeResourceObservation{entity_id:"svc".into(),repository:Some("r".into()),cpu_ms:Some(30.0),memory_bytes:Some(300),latency_ms:Some(40.0),error_rate:Some(0.3),sample_count:10,timestamp_ms:Some(2),trace_id:Some("t2".into()),metadata:BTreeMap::new()},
        ]);
        let hotspot=engine.runtime_hotspots().into_iter().find(|h|h.entity_id=="svc").unwrap();
        assert!((hotspot.cpu_ms-20.0).abs()<0.001);
        assert_eq!(hotspot.memory_bytes,200);
    }
}
