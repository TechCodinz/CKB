//! Unified Deep Causality evidence bundle construction.

use super::{CausalArtifactExtractor, CausalGraphAdapter, DeepCausalityEngine, RepositoryArtifact};
use crate::graph::DependencyGraph;

#[path = "deep_causality_workspace.rs"]
pub mod workspace;
pub use workspace::*;

/// Fuse CKB's authoritative dependency/runtime graph with repository artifact
/// evidence. The adapter preserves existing graph/runtime identity; artifact
/// extraction enriches it with schema/infra/config/security/event/ownership
/// facts. No evidence class is upgraded merely because two sources agree.
pub fn build_deep_causality_bundle(
    graph: &DependencyGraph,
    repository: impl Into<String>,
    artifacts: &[RepositoryArtifact],
) -> DeepCausalityEngine {
    let repository = repository.into();
    let mut engine = CausalGraphAdapter::new(graph).repository(repository).build();
    CausalArtifactExtractor::enrich(&mut engine, artifacts);
    engine
}

/// Merge an externally prepared evidence engine into an existing engine while
/// preserving the original fact evidence class/confidence. Unknown references
/// are rejected by `add_fact` rather than silently creating entities.
pub fn merge_deep_causality_evidence(target: &mut DeepCausalityEngine, source: &DeepCausalityEngine) -> Result<(), String> {
    for entity in source.entities() {
        target.upsert_entity(entity.clone());
    }
    for fact in source.facts() {
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
}
