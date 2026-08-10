//! Unified Deep Causality evidence bundle construction.

use super::{CausalArtifactExtractor, CausalGraphAdapter, DeepCausalityEngine, RepositoryArtifact};
use crate::graph::DependencyGraph;

#[path = "deep_causality_workspace.rs"]
pub mod workspace;
pub use workspace::*;

#[path = "deep_causality_artifacts_v2_entry.rs"]
mod artifacts_v2_entry;

#[path = "deep_causality_events.rs"]
mod events;

#[path = "deep_causality_contract_fields.rs"]
mod contract_fields;

#[path = "deep_causality_manifests.rs"]
mod manifests;

#[path = "deep_causality_infra_refs.rs"]
mod infra_refs;

#[path = "deep_causality_federation.rs"]
pub mod federation;
pub use federation::*;

#[path = "deep_causality_runtime.rs"]
pub mod runtime;
pub use runtime::*;

#[path = "deep_causality_contracts.rs"]
pub mod contracts;
pub use contracts::*;

#[path = "deep_causality_human.rs"]
pub mod human;
pub use human::*;

#[path = "memory_lane.rs"]
pub mod memory_lane;
pub use memory_lane::*;

#[path = "memory_lane_store.rs"]
pub mod memory_lane_store;
pub use memory_lane_store::*;

/// Fuse CKB's authoritative dependency/runtime graph with repository artifact
/// evidence. The adapter preserves existing graph/runtime identity; baseline
/// artifact extraction is followed by precision contract/ORM/infra/test
/// extraction, field-level contract typing, package-manifest federation facts,
/// explicit Terraform references, and shared event/topic/queue identity. No
/// evidence class is upgraded merely because multiple sources agree.
pub fn build_deep_causality_bundle(
    graph: &DependencyGraph,
    repository: impl Into<String>,
    artifacts: &[RepositoryArtifact],
) -> DeepCausalityEngine {
    let repository = repository.into();
    let mut engine = CausalGraphAdapter::new(graph).repository(repository).build();
    CausalArtifactExtractor::enrich(&mut engine, artifacts);
    artifacts_v2_entry::enrich_deep_artifact_semantics(&mut engine, artifacts);
    manifests::enrich_extra_manifests(&mut engine, artifacts);
    contract_fields::enrich_contract_fields(&mut engine, artifacts);
    infra_refs::enrich_infrastructure_references(&mut engine, artifacts);
    events::enrich_event_identity(&mut engine, artifacts);
    engine
}

/// Merge an externally prepared evidence engine into an existing engine while
/// preserving evidence classes/confidence and without downgrading a richer
/// entity already present in the target federation.
pub fn merge_deep_causality_evidence(target: &mut DeepCausalityEngine, source: &DeepCausalityEngine) -> Result<(), String> {
    for entity in source.entities() {
        if let Some(existing) = target.entities().find(|candidate| candidate.id == entity.id).cloned() {
            let mut merged = existing;
            if matches!(&merged.kind, super::CausalEntityKind::Unknown) && !matches!(&entity.kind, super::CausalEntityKind::Unknown) {
                merged.kind = entity.kind.clone();
            }
            if merged.name.is_empty() && !entity.name.is_empty() { merged.name = entity.name.clone(); }
            if merged.repository.is_none() { merged.repository = entity.repository.clone(); }
            if merged.path.is_none() { merged.path = entity.path.clone(); }
            for (key, value) in &entity.attributes { merged.attributes.entry(key.clone()).or_insert_with(|| value.clone()); }
            target.upsert_entity(merged);
        } else {
            target.upsert_entity(entity.clone());
        }
    }
    for fact in source.facts() {
        if target.facts().iter().any(|existing| existing == fact) { continue; }
        target.add_fact(fact.clone())?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_graph_can_still_receive_explicit_artifact_evidence() {
        let graph = DependencyGraph::new();
        let artifacts = vec![RepositoryArtifact {
            repository: "acme/api".into(),
            path: "prisma/schema.prisma".into(),
            content: "model User {\n id String @id\n}".into(),
        }];
        let engine = build_deep_causality_bundle(&graph, "acme/api", &artifacts);
        assert!(engine.entities().any(|entity| entity.name == "User"));
    }

    #[test]
    fn high_fidelity_pass_extracts_graphql_contracts() {
        let graph = DependencyGraph::new();
        let artifacts = vec![RepositoryArtifact {
            repository: "acme/api".into(),
            path: "schema.graphql".into(),
            content: "type User {\n id: ID!\n}".into(),
        }];
        let engine = build_deep_causality_bundle(&graph, "acme/api", &artifacts);
        assert!(engine.entities().any(|entity| entity.kind == crate::analysis::CausalEntityKind::Schema && entity.name == "User"));
        let contracts = derive_contract_snapshots(&engine);
        assert!(contracts.iter().any(|snapshot| snapshot.entity_id == "repo:acme/api::schema:User" && snapshot.contract.fields.iter().any(|field| field.name == "id" && field.required)));
    }

    #[test]
    fn producer_and_consumer_share_explicit_event_identity() {
        let graph = DependencyGraph::new();
        let artifacts = vec![
            RepositoryArtifact { repository:"acme/api".into(), path:"src/publisher.ts".into(), content:"bus.publish(\"orders.created\", value);".into() },
            RepositoryArtifact { repository:"acme/api".into(), path:"src/consumer.ts".into(), content:"bus.subscribe(\"orders.created\", handler);".into() },
        ];
        let engine = build_deep_causality_bundle(&graph, "acme/api", &artifacts);
        assert_eq!(engine.entities().filter(|entity| entity.id == "event:orders.created").count(), 1);
        assert!(engine.distributed_flow("repo:acme/api::file:src/publisher.ts", "repo:acme/api::file:src/consumer.ts", 4).is_some());
    }

    #[test]
    fn memory_lane_is_project_bounded_and_guarded() {
        let lane = MemoryLaneEngine::new("acme/api");
        assert_eq!(lane.profile.project_id, "acme/api");
        assert_eq!(lane.version, MEMORY_LANE_VERSION);
    }
}
