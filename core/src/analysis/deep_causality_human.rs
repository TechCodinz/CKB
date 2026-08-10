//! HUMAN evidence ingestion for review/ownership systems.
//!
//! These observations are supplied by an external SCM/review integration and
//! are never inferred from commit authorship. This keeps HUMAN and HISTORY truth
//! distinct while still allowing socio-technical risk analysis to combine them.

use crate::analysis::deep_causality::*;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HumanReviewObservation {
    pub reviewer: String,
    pub entity_id: String,
    #[serde(default)] pub repository: Option<String>,
    #[serde(default)] pub reviewed_at_ms: Option<i64>,
    #[serde(default)] pub source: Option<String>,
    #[serde(default)] pub metadata: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HumanOwnershipObservation {
    pub owner: String,
    pub entity_id: String,
    #[serde(default)] pub repository: Option<String>,
    #[serde(default)] pub team: bool,
    #[serde(default)] pub metadata: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct HumanEvidenceIngestReport {
    pub reviews_ingested: usize,
    pub ownerships_ingested: usize,
}

pub fn ingest_human_evidence(
    engine: &mut DeepCausalityEngine,
    reviews: &[HumanReviewObservation],
    ownerships: &[HumanOwnershipObservation],
) -> HumanEvidenceIngestReport {
    let mut reviews_ingested = 0usize;
    let mut ownerships_ingested = 0usize;

    for observation in reviews {
        let owner_id = format!("owner:review:{}", normalize_identity(&observation.reviewer));
        ensure_owner(engine, &owner_id, &observation.reviewer, false);
        ensure_target(engine, &observation.entity_id, observation.repository.as_deref());
        let mut metadata = observation.metadata.clone();
        if let Some(source) = observation.source.as_ref() { metadata.insert("review.source".into(), source.clone()); }
        let fact = CausalFact {
            from: owner_id,
            to: observation.entity_id.clone(),
            relation: CausalRelationKind::Reviews,
            evidence: CausalEvidenceClass::Human,
            confidence: 1.0,
            condition: None,
            timestamp_ms: observation.reviewed_at_ms,
            metadata,
        };
        if add_unique(engine, fact) { reviews_ingested += 1; }
    }

    for observation in ownerships {
        let prefix = if observation.team { "team" } else { "owner" };
        let owner_id = format!("{prefix}:human:{}", normalize_identity(&observation.owner));
        ensure_owner(engine, &owner_id, &observation.owner, observation.team);
        ensure_target(engine, &observation.entity_id, observation.repository.as_deref());
        let fact = CausalFact {
            from: owner_id,
            to: observation.entity_id.clone(),
            relation: CausalRelationKind::Owns,
            evidence: CausalEvidenceClass::Human,
            confidence: 1.0,
            condition: None,
            timestamp_ms: None,
            metadata: observation.metadata.clone(),
        };
        if add_unique(engine, fact) { ownerships_ingested += 1; }
    }

    HumanEvidenceIngestReport { reviews_ingested, ownerships_ingested }
}

fn ensure_owner(engine: &mut DeepCausalityEngine, id: &str, name: &str, team: bool) {
    if engine.entities().any(|entity| entity.id == id) { return; }
    engine.upsert_entity(CausalEntity {
        id: id.to_string(),
        kind: if team { CausalEntityKind::Team } else { CausalEntityKind::Owner },
        name: name.to_string(),
        repository: None,
        path: None,
        attributes: BTreeMap::new(),
    });
}

fn ensure_target(engine: &mut DeepCausalityEngine, id: &str, repository: Option<&str>) {
    if engine.entities().any(|entity| entity.id == id) { return; }
    engine.upsert_entity(CausalEntity {
        id: id.to_string(), kind: CausalEntityKind::Unknown, name: id.to_string(),
        repository: repository.map(str::to_string), path: None,
        attributes: BTreeMap::from([("created.by".into(), "human_evidence_import".into())]),
    });
}

fn add_unique(engine: &mut DeepCausalityEngine, fact: CausalFact) -> bool {
    if engine.facts().iter().any(|existing| existing == &fact) { return false; }
    engine.add_fact(fact).is_ok()
}

fn normalize_identity(value: &str) -> String {
    value.trim().to_ascii_lowercase().chars()
        .map(|character| if character.is_ascii_alphanumeric() || matches!(character, '@' | '.' | '-' | '_') { character } else { '_' })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn review_observation_remains_human_evidence() {
        let mut engine=DeepCausalityEngine::new();
        let report=ingest_human_evidence(&mut engine,&[HumanReviewObservation{reviewer:"Ada".into(),entity_id:"file".into(),repository:Some("r".into()),reviewed_at_ms:Some(1),source:Some("github-pr".into()),metadata:BTreeMap::new()}],&[]);
        assert_eq!(report.reviews_ingested,1);
        assert_eq!(engine.facts()[0].evidence,CausalEvidenceClass::Human);
    }
}
