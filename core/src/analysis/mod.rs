//! Architecture pattern detection and drift analysis

mod boundaries;
mod patterns;
mod drift;
pub mod rules;
pub mod clone_detector;
pub mod test_coverage;
pub mod memory;
pub mod causal;
pub mod activity;
pub mod intelligence_fabric;
pub mod incremental;
pub mod query_language;
pub mod frontier_model_profile;
pub mod frontier_model_registry;
pub mod frontier_model_selector;
pub mod deep_causality;
pub mod deep_causality_adapter;
pub mod deep_causality_extractors;
pub mod deep_causality_advanced;
pub mod deep_causality_bundle;
pub mod deep_causality_history;
pub mod live_execution;
pub mod runtime_diagnostics;
pub mod evolution_memory;
pub mod reference_architecture;

pub use boundaries::*;
pub use patterns::*;
pub use drift::*;
pub use rules::*;
pub use clone_detector::*;
pub use test_coverage::*;
pub use memory::*;
pub use causal::*;
pub use activity::*;
pub use intelligence_fabric::*;
pub use incremental::*;
pub use query_language::*;
pub use frontier_model_profile::*;
pub use frontier_model_registry::*;
pub use frontier_model_selector::*;
pub use deep_causality::*;
pub use deep_causality_adapter::*;
pub use deep_causality_extractors::*;
#[allow(unused_imports)]
pub use deep_causality_advanced::*;
pub use deep_causality_bundle::*;
pub use deep_causality_history::*;
pub use live_execution::*;
pub use runtime_diagnostics::*;
pub use evolution_memory::*;
pub use reference_architecture::*;

use crate::graph::DependencyGraph;
use crate::types::*;
use anyhow::Result;
use std::collections::HashSet;

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

        for cycle in graph.find_architectural_cycles()? {
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

    /// Structural delta between two snapshot graphs: which files, nodes
    /// and edges appeared/disappeared, and which violations were resolved
    /// or introduced. Violations are recomputed from each graph (stored
    /// snapshots keep the graph, not the scan report), keyed on
    /// (kind, from, to, boundary) because violation IDs are fresh UUIDs
    /// per scan.
    ///
    /// Node identity is normalized to paths relative to each graph's own
    /// common root: two scans of the same project from different absolute
    /// locations (e.g. `fixture-a/` vs `fixture-b/` checkouts) must diff
    /// as the same project, not as "everything removed, everything
    /// added".
    pub fn compare_graphs(&self, before: &DependencyGraph, after: &DependencyGraph) -> Result<SnapshotDelta> {
        let before_root = graph_common_root(before);
        let after_root = graph_common_root(after);

        let before_files: HashSet<String> = file_paths_relative(before, &before_root);
        let after_files: HashSet<String> = file_paths_relative(after, &after_root);

        let before_nodes: HashSet<String> = before
            .nodes()
            .iter()
            .map(|n| relative_id(&n.id.0, &before_root))
            .collect();
        let after_nodes: HashSet<String> = after
            .nodes()
            .iter()
            .map(|n| relative_id(&n.id.0, &after_root))
            .collect();

        let before_edges = edge_multiset(before, &before_root);
        let after_edges = edge_multiset(after, &after_root);

        let before_violations = self.violation_set(before, &before_root)?;
        let after_violations = self.violation_set(after, &after_root)?;
        let before_keys: HashSet<String> = before_violations.iter().map(|v| v.key()).collect();
        let after_keys: HashSet<String> = after_violations.iter().map(|v| v.key()).collect();

        Ok(SnapshotDelta {
            files_before: before_files.len(),
            files_after: after_files.len(),
            files_added: sorted_diff(&after_files, &before_files),
            files_removed: sorted_diff(&before_files, &after_files),
            nodes_before: before.node_count(),
            nodes_after: after.node_count(),
            nodes_added: sorted_diff(&after_nodes, &before_nodes),
            nodes_removed: sorted_diff(&before_nodes, &after_nodes),
            edges_before: before.edge_count(),
            edges_after: after.edge_count(),
            edges_added: multiset_diff(&after_edges, &before_edges),
            edges_removed: multiset_diff(&before_edges, &after_edges),
            violations_before: before_violations.len(),
            violations_after: after_violations.len(),
            violations_resolved: before_violations
                .iter()
                .filter(|v| !after_keys.contains(&v.key()))
                .cloned()
                .collect(),
            violations_introduced: after_violations
                .iter()
                .filter(|v| !before_keys.contains(&v.key()))
                .cloned()
                .collect(),
        })
    }

    /// Recompute a graph's violations and normalize them for diffing.
    fn violation_set(&self, graph: &DependencyGraph, root: &std::path::Path) -> Result<Vec<ViolationDelta>> {
        let patterns = self.detect_patterns(graph)?;
        let drift = self.detect_drift(graph, &patterns)?;
        let mut out: Vec<ViolationDelta> = drift
            .iter()
            .map(|v| ViolationDelta {
                kind: format!("{:?}", v.kind),
                from: relative_id(&v.from.0, root),
                to: relative_id(&v.to.0, root),
                boundary: v.boundary.clone(),
                severity: format!("{:?}", v.severity),
                message: v.message.clone(),
            })
            .collect();
        out.sort_by_key(|a| a.key());
        out.dedup_by(|a, b| a.key() == b.key());
        Ok(out)
    }

    pub fn infer_boundaries(&self, graph: &DependencyGraph) -> Result<Vec<ArchitectureBoundary>> {
        let mut boundaries = Vec::new();
        for inferencer in &self.boundary_inferencers {
            boundaries.extend(inferencer.infer(graph)?);
        }
        Ok(boundaries)
    }

    pub fn slice_context_for_prompt(&self, graph: &DependencyGraph, file: &str, depth: usize) -> Result<String> {
        // Node IDs in a scanned graph embed the path exactly as the scanner
        // saw it (frequently absolute), while callers of this tool pass the
        // natural workspace-relative path ("src/core/util.ts"). An exact
        // `"{file}::file"` lookup therefore missed every real node and the
        // tool returned an empty context for the files that matter most.
        // Resolve like the impact path does: exact ID first, then a File
        // node whose stored path equals or ends with the requested path.
        let node_id = resolve_file_node(graph, file);
        let depth = depth.clamp(1, 5);

        let (callers, deps) = match &node_id {
            Some(id) => (
                collect_neighbors(graph, id, depth, true),
                collect_neighbors(graph, id, depth, false),
            ),
            None => (Vec::new(), Vec::new()),
        };

        let mut out = String::new();
        out.push_str(&format!("<architectural_context target_file=\"{}\">\n", file));
        out.push_str("  <incoming_dependents>\n");
        for (caller, d) in callers {
            out.push_str(&format!("    <dependent id=\"{}\" depth=\"{}\"/>\n", caller.0, d));
        }
        out.push_str("  </incoming_dependents>\n");
        out.push_str("  <outgoing_dependencies>\n");
        for (dep, d) in deps {
            out.push_str(&format!("    <dependency id=\"{}\" depth=\"{}\"/>\n", dep.0, d));
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
            doc.push('\n');
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

impl Default for ArchitectureAnalyzer {
    fn default() -> Self {
        Self::new()
    }
}

/// One edge in a snapshot delta. `count` covers parallel identical
/// edges (same endpoints and kind) whose multiplicity changed.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct EdgeDelta {
    pub from: String,
    pub to: String,
    pub kind: String,
    pub count: usize,
}

/// One violation in a snapshot delta, with root-relative node IDs.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ViolationDelta {
    pub kind: String,
    pub from: String,
    pub to: String,
    pub boundary: String,
    pub severity: String,
    pub message: String,
}

impl ViolationDelta {
    /// Identity for diffing: violation UUIDs are per-scan noise, so two
    /// violations are "the same" when kind/endpoints/boundary agree.
    fn key(&self) -> String {
        format!("{}|{}|{}|{}", self.kind, self.from, self.to, self.boundary)
    }
}

/// The full structural delta between two snapshot graphs, as produced by
/// `ArchitectureAnalyzer::compare_graphs`. All node/file identifiers are
/// relative to each graph's own scan root (see `graph_common_root`).
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct SnapshotDelta {
    pub files_before: usize,
    pub files_after: usize,
    pub files_added: Vec<String>,
    pub files_removed: Vec<String>,
    pub nodes_before: usize,
    pub nodes_after: usize,
    pub nodes_added: Vec<String>,
    pub nodes_removed: Vec<String>,
    pub edges_before: usize,
    pub edges_after: usize,
    pub edges_added: Vec<EdgeDelta>,
    pub edges_removed: Vec<EdgeDelta>,
    pub violations_before: usize,
    pub violations_after: usize,
    pub violations_resolved: Vec<ViolationDelta>,
    pub violations_introduced: Vec<ViolationDelta>,
}

/// Deepest common directory prefix of every node path in the graph —
/// the scan root the graph's absolute node IDs are relative to. With a
/// single distinct path this is that file's parent directory.
fn graph_common_root(graph: &DependencyGraph) -> std::path::PathBuf {
    let mut paths: Vec<std::path::PathBuf> =
        graph.nodes().iter().map(|n| n.path.clone()).collect();
    paths.sort();
    paths.dedup();
    if paths.is_empty() {
        return std::path::PathBuf::new();
    }
    let mut root = std::path::PathBuf::new();
    for component in paths[0].components() {
        let candidate = root.join(component.as_os_str());
        if paths.iter().all(|p| p.starts_with(&candidate)) {
            root = candidate;
        } else {
            break;
        }
    }
    if paths.len() == 1 || root == paths[0] {
        root = paths[0].parent().map(|p| p.to_path_buf()).unwrap_or_default();
    }
    root
}

/// Rewrite an absolute node ID (`<abs path>::<symbol>`) relative to the
/// graph root (`<rel path>::<symbol>`). IDs without a `::` separator or
/// outside the root pass through unchanged.
fn relative_id(raw: &str, root: &std::path::Path) -> String {
    let root_str = root.to_string_lossy();
    match raw.split_once("::") {
        Some((path, rest)) => match path.strip_prefix(root_str.as_ref()) {
            Some(rel) => format!("{}::{}", rel.trim_start_matches(['/', '\\']), rest),
            None => raw.to_string(),
        },
        None => raw.to_string(),
    }
}

/// Resolve a caller-supplied file reference to the File node the scanner
/// stored. Exact `"{file}::file"` ID match first (the historical behaviour
/// for callers passing scanner-shaped paths), then a path match: the
/// stored path equals the request or ends with `/{request}` — the same
/// normalization `find_affected_nodes` applies for impact analysis.
fn resolve_file_node(graph: &DependencyGraph, file: &str) -> Option<NodeId> {
    let exact = NodeId(format!("{}::file", file));
    if graph.nodes().iter().any(|n| n.id == exact) {
        return Some(exact);
    }
    let normalized = file.replace('\\', "/");
    graph
        .nodes()
        .iter()
        .filter(|n| n.kind == NodeKind::File)
        .find(|n| {
            let path = n.path.to_string_lossy().replace('\\', "/");
            path == normalized || path.ends_with(&format!("/{}", normalized))
        })
        .map(|n| n.id.clone())
}

/// Breadth-first neighbourhood of `start` up to `depth` hops, in either
/// edge direction, returning each reached node with its hop distance.
/// Order is deterministic (BFS level order, graph iteration order within
/// a level) and each node appears once, at its minimum distance.
fn collect_neighbors(
    graph: &DependencyGraph,
    start: &NodeId,
    depth: usize,
    incoming: bool,
) -> Vec<(NodeId, usize)> {
    let mut out: Vec<(NodeId, usize)> = Vec::new();
    let mut seen: std::collections::HashSet<NodeId> = std::collections::HashSet::new();
    seen.insert(start.clone());
    let mut frontier: Vec<NodeId> = vec![start.clone()];
    for level in 1..=depth {
        let mut next: Vec<NodeId> = Vec::new();
        for node in &frontier {
            let neighbors = if incoming {
                graph.get_callers(node)
            } else {
                graph.get_dependencies(node).unwrap_or_default()
            };
            for neighbor in neighbors {
                if seen.insert(neighbor.clone()) {
                    out.push((neighbor.clone(), level));
                    next.push(neighbor);
                }
            }
        }
        frontier = next;
        if frontier.is_empty() {
            break;
        }
    }
    out
}

/// Root-relative paths of all File nodes in the graph.
fn file_paths_relative(
    graph: &DependencyGraph,
    root: &std::path::Path,
) -> HashSet<String> {
    graph
        .nodes()
        .iter()
        .filter(|n| n.kind == NodeKind::File)
        .map(|n| {
            let rel = relative_id(&n.id.0, root);
            rel.strip_suffix("::file").map(|s| s.to_string()).unwrap_or(rel)
        })
        .collect()
}

type EdgeKey = (String, String, String);

/// Edge multiset keyed on (relative from, relative to, kind) — edge
/// UUIDs are per-scan noise and parallel identical edges are possible.
fn edge_multiset(
    graph: &DependencyGraph,
    root: &std::path::Path,
) -> std::collections::HashMap<EdgeKey, usize> {
    let mut out: std::collections::HashMap<EdgeKey, usize> = std::collections::HashMap::new();
    for edge in graph.edges() {
        let key = (
            relative_id(&edge.from.0, root),
            relative_id(&edge.to.0, root),
            format!("{:?}", edge.kind),
        );
        *out.entry(key).or_insert(0) += 1;
    }
    out
}

fn sorted_diff(a: &HashSet<String>, b: &HashSet<String>) -> Vec<String> {
    let mut out: Vec<String> = a.difference(b).cloned().collect();
    out.sort();
    out
}

/// Entries present in `a` beyond their multiplicity in `b`.
fn multiset_diff(
    a: &std::collections::HashMap<EdgeKey, usize>,
    b: &std::collections::HashMap<EdgeKey, usize>,
) -> Vec<EdgeDelta> {
    let mut out: Vec<EdgeDelta> = a
        .iter()
        .filter_map(|((from, to, kind), count)| {
            let other = b.get(&(from.clone(), to.clone(), kind.clone())).copied().unwrap_or(0);
            (count > &other).then(|| EdgeDelta {
                from: from.clone(),
                to: to.clone(),
                kind: kind.clone(),
                count: count - other,
            })
        })
        .collect();
    out.sort_by(|x, y| (&x.from, &x.to, &x.kind).cmp(&(&y.from, &y.to, &y.kind)));
    out
}

pub trait PatternDetector: Send + Sync {
    fn detect(&self, graph: &DependencyGraph) -> Result<Option<ArchitecturalPattern>>;
}

pub trait BoundaryInferencer: Send + Sync {
    fn infer(&self, graph: &DependencyGraph) -> Result<Vec<ArchitectureBoundary>>;
}

impl crate::CkbEngine {
    pub fn new_with_storage_path(path: &str) -> Result<Self> {
        let parser = std::sync::Arc::new(crate::LanguageParser::new());
        let storage = std::sync::Arc::new(crate::GraphStorage::new(path)?);
        let graph = std::sync::Arc::new(tokio::sync::RwLock::new(DependencyGraph::new()));
        let analyzer = std::sync::Arc::new(ArchitectureAnalyzer::new());
        Ok(Self { parser, graph, analyzer, storage })
    }

    pub async fn reset_architecture_graph(&self) {
        *self.graph.write().await = DependencyGraph::new();
    }

    pub async fn restore_latest_architecture_snapshot(&self) -> Result<bool> {
        if let Some(graph) = self.storage.get_latest_snapshot().await? {
            *self.graph.write().await = graph;
            Ok(true)
        } else {
            Ok(false)
        }
    }

    pub async fn ingest_otlp_spans_exact(&self, raw_payload: &str) -> Result<crate::OtlpIngestReport> {
        let metrics = crate::OtlpReceiver::ingest_spans(raw_payload)?;
        let summary = crate::OtlpReceiver::summarize(&metrics);
        let mut graph = self.graph.write().await;
        for (node_id, runtime) in metrics {
            graph.record_runtime_metrics(node_id, runtime);
        }
        Ok(summary)
    }

    pub async fn architecture_graph_snapshot(&self) -> DependencyGraph {
        self.graph.read().await.clone()
    }

    pub async fn architecture_snapshot_metadata(&self) -> Result<Vec<crate::SnapshotMetadata>> {
        self.storage.list_snapshots().await
    }

    pub async fn architecture_snapshot_graph(&self, snapshot_id: &str) -> Result<Option<DependencyGraph>> {
        self.storage.load_snapshot(snapshot_id).await
    }
}
