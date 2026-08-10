//! Unified Deep Causality evidence bundle construction.

use super::{CausalArtifactExtractor, CausalGraphAdapter, DeepCausalityEngine, RepositoryArtifact};
use crate::graph::DependencyGraph;

#[path = "deep_causality_workspace.rs"]
pub mod workspace;
pub use workspace::*;

#[path = "deep_causality_artifacts_v2_entry.rs"]
mod artifacts_v2_entry;

#[path = "deep_causality_federation.rs"]
pub mod federation;
pub use federation::*;

#[path = "deep_causality_runtime.rs"]
pub mod runtime;
pub use runtime::*;

/// Fuse CKB's authoritative dependency/runtime graph with repository artifact
/// evidence. The adapter preserves existing graph/runtime identity; baseline
/// artifact extraction is followed by a higher-fidelity pass for explicit
/// contracts, package dependencies, ORM/SQL access, Compose dependencies,
/// config guards and test imports. No evidence class is upgraded merely because
/// multiple sources agree.
pub fn build_deep_causality_bundle(
    graph: &DependencyGraph,
    repository: impl Into<String>,
    artifacts: &[RepositoryArtifact],
) -> DeepCausalityEngine {
    let repository = repository.into();
    let mut engine = CausalGraphAdapter::new(graph).repository(repository).build();
    CausalArtifactExtractor::enrich(&mut engine, artifacts);
    artifacts_v2_entry::enrich_deep_artifact_semantics(&mut engine, artifacts);
    engine
}

/// Merge an externally prepared evidence engine into an existing engine while
/// preserving evidence classes/confidence and without downgrading a richer
/// entity already present in the target federation.
pub fn merge_deep_causality_evidence(target: &mut DeepCausalityEngine, source: &DeepCausalityEngine) -> Result<(), String> {
    for entity in source.entities() {
        if let Some(existing) = target.entities().find(|e| e.id == entity.id).cloned() {
            let mut merged = existing;
            if matches!(merged.kind, super::CausalEntityKind::Unknown) && !matches!(entity.kind, super::CausalEntityKind::Unknown) {
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
        assert!(engine.entities().any(|e| e.name == "User"));
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
        assert!(engine.entities().any(|e| e.kind == crate::analysis::CausalEntityKind::Schema && e.name == "User"));
    }
}
