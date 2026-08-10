//! Architecture pattern detection and drift analysis

mod boundaries;
mod patterns;
mod drift;
pub mod clone_detector;
pub mod test_coverage;
pub mod memory;
pub mod causal;
pub mod activity;
pub mod intelligence_fabric;

pub use boundaries::*;
pub use patterns::*;
pub use drift::*;
pub use clone_detector::*;
pub use test_coverage::*;
pub use memory::*;
pub use causal::*;
pub use activity::*;
pub use intelligence_fabric::*;

use crate::graph::DependencyGraph;
use crate::types::*;
use anyhow::Result;
use std::collections::HashSet;

// Reality sessions and architecture snapshots need an owned graph while the
// live graph remains behind an RwLock. DependencyGraph is already fully
// bincode/serde-persistable, so use that same proven representation for an
// internal snapshot clone instead of exposing or duplicating its private
// petgraph/index bookkeeping. This also keeps every clone structurally
// consistent with the on-disk representation CKB restores later.
impl Clone for DependencyGraph {
    fn clone(&self) -> Self {
        let bytes = bincode::serialize(self)
            .expect("CKB DependencyGraph serialization must succeed for snapshot cloning");
        bincode::deserialize(&bytes)
            .expect("CKB DependencyGraph deserialization must succeed for snapshot cloning")
    }
}

impl std::fmt::Debug for DependencyGraph {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DependencyGraph")
            .field("nodes", &self.node_count())
            .field("edges", &self.edge_count())
            .finish_non_exhaustive()
    }
}

pub struct ArchitectureAnalyzer {
    pattern_detectors: Vec<Box<dyn PatternDetector>>,
    boundary_inferencers: Vec<Box<dyn BoundaryInferencer>>,
}

impl ArchitectureAnalyzer {
    pub fn new() -> Self {
        Self {
            pattern_detectors: vec![
                Box::new(patterns::LayeredArchitectureDetector::new()),
                Box::new(patterns::ModularArchitectureDetector),
                Box::new(patterns::HexagonalArchitectureDetector),
            ],
            boundary_inferencers: vec![
                Box::new(boundaries::PathBasedInferencer),
                Box::new(boundaries::NamingBasedInferencer),
                Box::new(boundaries::AnnotationBasedInferencer),
            ],
        }
    }

    pub fn detect_patterns(&self, graph: &DependencyGraph) -> Result<Vec<ArchitecturalPattern>> {
        let mut patterns = Vec::new();
        for detector in &self.pattern_detectors {
            if let Some(pattern) = detector.detect(graph)? {
                patterns.push(pattern);
            }
        }
        Ok(patterns)
    }

    pub fn detect_drift(&self, graph: &DependencyGraph, patterns: &[ArchitecturalPattern]) -> Result<Vec<DriftViolation>> {
        let mut violations = Vec::new();
        let mut seen = HashSet::new();

        for pattern in patterns {
            for boundary in &pattern.boundaries {
                for v in self.check_boundary(graph, boundary)? {
                    let key = format!("{:?}|{}|{}|{}", v.kind, v.from.0, v.to.0, v.boundary);
                    if seen.insert(key) { violations.push(v); }
                }
            }
        }

        for boundary in self.infer_boundaries(graph)? {
            for v in self.check_boundary(graph, &boundary)? {
                let key = format!("{:?}|{}|{}|{}", v.kind, v.from.0, v.to.0, v.boundary);
                if seen.insert(key) { violations.push(v); }
            }
        }

        for cycle in graph.find_cycles()? {
            if cycle.len() < 2 { continue; }
            for pair in cycle.windows(2) {
                let from = pair[0].clone();
                let to = pair[1].clone();
                let key = format!("cycle|{}|{}", from.0, to.0);
                if seen.insert(key) {
                    violations.push(DriftViolation {
                        id: uuid::Uuid::new_v4(),
                        kind: ViolationKind::CircularDependency,
                        from,
                        to,
                        boundary: "dependency graph".to_string(),
                        message: "Circular dependency detected from strongly connected graph component.".to_string(),
                        severity: Severity::Error,
                        suggested_fix: Some("Break the cycle by extracting a stable contract/interface or inverting one dependency.".to_string()),
                    });
                }
            }
        }

        Ok(violations)
    }

    pub fn infer_boundaries(&self, graph: &DependencyGraph) -> Result<Vec<ArchitecturalBoundary>> {
        let mut boundaries = Vec::new();
        for inferencer in &self.boundary_inferencers {
            boundaries.extend(inferencer.infer(graph)?);
        }
        Ok(boundaries)
    }

    fn check_boundary(&self, graph: &DependencyGraph, boundary: &ArchitecturalBoundary) -> Result<Vec<DriftViolation>> {
        let mut violations = Vec::new();
        for edge in graph.edges() {
            let from = graph.get_node(&edge.from);
            let to = graph.get_node(&edge.to);
            let (Some(from), Some(to)) = (from, to) else { continue };
            for rule in &boundary.rules {
                if rule.matches(from, to) && !rule.allowed {
                    violations.push(DriftViolation {
                        id: uuid::Uuid::new_v4(),
                        kind: ViolationKind::BoundaryViolation,
                        from: edge.from.clone(),
                        to: edge.to.clone(),
                        boundary: boundary.name.clone(),
                        message: format!("Dependency violates architecture boundary: {}", boundary.name),
                        severity: Severity::Warning,
                        suggested_fix: Some("Move the dependency behind an allowed boundary or introduce an interface/adapter.".to_string()),
                    });
                }
            }
        }
        Ok(violations)
    }
}

impl Default for ArchitectureAnalyzer {
    fn default() -> Self { Self::new() }
}
