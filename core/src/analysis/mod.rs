//! Architecture pattern detection and drift analysis

mod boundaries;
mod patterns;
mod drift;
pub mod clone_detector;
pub mod test_coverage;

pub use boundaries::*;
pub use patterns::*;
pub use drift::*;
pub use clone_detector::*;
pub use test_coverage::*;

use crate::graph::DependencyGraph;
use crate::types::*;
use anyhow::Result;
use std::collections::HashSet;

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

        // Also enforce independently inferred boundaries. Pattern detectors do
        // not always emit every naming/annotation boundary, so relying only on
        // pattern-owned boundaries previously left check_boundary effectively
        // unreachable for many real projects.
        for boundary in self.infer_boundaries(graph)? {
            for v in self.check_boundary(graph, &boundary)? {
                let key = format!("{:?}|{}|{}|{}", v.kind, v.from.0, v.to.0, v.boundary);
                if seen.insert(key) { violations.push(v); }
            }
        }

        // Cycles are graph facts, not heuristic guesses.
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

    pub fn infer_boundaries(&self, graph: &DependencyGraph) -> Result<Vec<ArchitectureBoundary>> {
        let mut boundaries = Vec::new();
        for inferencer in &self.boundary_inferencers {
            boundaries.extend(inferencer.infer(graph)?);
        }
        Ok(boundaries)
    }

    pub fn slice_context_for_prompt(&self, graph: &DependencyGraph, file: &str, _depth: usize) -> Result<String> {
        let node_id = NodeId(format!("{}::file", file));
        let callers = graph.get_callers(&node_id);
        let deps = graph.get_dependencies(&node_id)?;

        let mut out = String::new();
        out.push_str(&format!("<architectural_context target_file=\"{}\">\n", file));
        out.push_str("  <incoming_dependents>\n");
        for caller in callers {
            out.push_str(&format!("    <dependent id=\"{}\"/>\n", caller.0));
        }
        out.push_str("  </incoming_dependents>\n");
        out.push_str("  <outgoing_dependencies>\n");
        for dep in deps {
            out.push_str(&format!("    <dependency id=\"{}\"/>\n", dep.0));
        }
        out.push_str("  </outgoing_dependencies>\n");
        out.push_str("</architectural_context>");
        Ok(out)
    }

    pub fn generate_ai_guidelines(&self, graph: &DependencyGraph) -> Result<String> {
        let boundaries = self.infer_boundaries(graph)?;
        let mut doc = String::new();
        doc.push_str("# Auto-Generated CKB Architectural Guidelines\n\n");
        doc.push_str("> Generated from the current dependency graph and inferred boundaries. Verify inferred policy before enforcing it as an organizational rule.\n\n");
        doc.push_str("## Inferred Architectural Boundaries\n\n");

        for b in boundaries {
            doc.push_str(&format!("### Boundary: {}\n", b.name));
            doc.push_str(&format!("- **Kind**: {:?}\n", b.kind));
            if !b.allowed_dependencies.is_empty() {
                doc.push_str(&format!("- **Allowed dependency tokens**: {}\n", b.allowed_dependencies.join(", ")));
            }
            if !b.forbidden_dependencies.is_empty() {
                doc.push_str(&format!("- **Forbidden dependency tokens**: {}\n", b.forbidden_dependencies.join(", ")));
            }
            doc.push_str("\n");
        }

        doc.push_str("## Guardrails\n");
        doc.push_str("1. Maintain separation of concerns between core logic, backend API, and presentation layers.\n");
        doc.push_str("2. Do not introduce circular dependencies across modules.\n");
        doc.push_str("3. Query CKB blast-radius analysis before deleting or changing public interfaces.\n");
        Ok(doc)
    }

    pub fn suggest_decoupling_refactor(&self, graph: &DependencyGraph, cycle_nodes: &[NodeId]) -> Result<String> {
        let mut refactor_plan = String::new();
        refactor_plan.push_str("CKB Decoupling Recommendation\n\n");
        refactor_plan.push_str("Evidence: cycle nodes discovered in the current dependency graph.\n\n");

        for (idx, node) in cycle_nodes.iter().enumerate() {
            let incoming = graph.incoming_degree(node)?;
            let outgoing = graph.outgoing_degree(node)?;
            refactor_plan.push_str(&format!(
                "{}. `{}` — fan-in {}, fan-out {}. Consider extracting the smallest stable contract that allows one cycle edge to be inverted or removed.\n",
                idx + 1, node.0, incoming, outgoing
            ));
        }
        refactor_plan.push_str("\nCKB does not claim the cycle is removed until the proposed patch is applied and the graph is rescanned.");
        Ok(refactor_plan)
    }

    /// Evidence-backed heuristic risk index. This is not presented as a
    /// learned failure probability: it combines observed graph centrality and,
    /// when available, real runtime error/hotpath telemetry.
    pub fn predict_failure_probability(&self, graph: &DependencyGraph, file: &str) -> Result<f32> {
        let file_node = NodeId(format!("{}::file", file));
        let inc = graph.incoming_degree(&file_node)? as f32;
        let out = graph.outgoing_degree(&file_node)? as f32;
        let structural = ((inc * 2.0 + out) / 30.0).min(0.65);

        let runtime = graph.get_runtime_metrics(&file_node).map(|m| {
            let error_component = (m.error_rate * 2.5).min(0.20);
            let hotpath_component = if m.is_hotpath { 0.10 } else { 0.0 };
            let latency_component = (m.avg_latency_ms / 5000.0).min(0.05);
            error_component + hotpath_component + latency_component
        }).unwrap_or(0.0);

        Ok((structural + runtime).min(0.99))
    }

    fn check_boundary(&self, graph: &DependencyGraph, boundary: &ArchitectureBoundary) -> Result<Vec<DriftViolation>> {
        if boundary.forbidden_dependencies.is_empty() {
            return Ok(Vec::new());
        }

        let nodes_by_id: std::collections::HashMap<NodeId, &Node> = graph.nodes()
            .into_iter()
            .map(|n| (n.id.clone(), n))
            .collect();
        let mut violations = Vec::new();

        for edge in graph.edges() {
            if !boundary.nodes.contains(&edge.from) { continue; }
            let Some(target) = nodes_by_id.get(&edge.to) else { continue; };
            if boundary.nodes.contains(&edge.to) { continue; }

            let target_text = format!("{} {}", target.name, target.path.to_string_lossy()).to_ascii_lowercase();
            let forbidden = boundary.forbidden_dependencies.iter()
                .find(|token| target_text.contains(&token.to_ascii_lowercase()));

            if let Some(token) = forbidden {
                violations.push(DriftViolation {
                    id: uuid::Uuid::new_v4(),
                    kind: ViolationKind::ForbiddenDependency,
                    from: edge.from.clone(),
                    to: edge.to.clone(),
                    boundary: boundary.name.clone(),
                    message: format!(
                        "{} crosses inferred boundary `{}` into forbidden dependency class `{}` via {:?} edge.",
                        edge.from.0, boundary.name, token, edge.kind
                    ),
                    severity: Severity::Warning,
                    suggested_fix: Some(format!(
                        "Introduce or depend on an allowed contract instead of directly coupling `{}` to `{}`.",
                        edge.from.0, edge.to.0
                    )),
                });
            }
        }
        Ok(violations)
    }
}

pub trait PatternDetector: Send + Sync {
    fn detect(&self, graph: &DependencyGraph) -> Result<Option<ArchitecturalPattern>>;
}

pub trait BoundaryInferencer: Send + Sync {
    fn infer(&self, graph: &DependencyGraph) -> Result<Vec<ArchitectureBoundary>>;
}
