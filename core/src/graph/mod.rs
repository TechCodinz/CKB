//! Dependency graph construction and querying

use std::collections::{HashMap, HashSet, VecDeque};
use std::path::{Component, Path};
use petgraph::graph::{DiGraph, NodeIndex};
use crate::types::*;
use crate::parser::FileAnalysis;
use anyhow::Result;

#[derive(serde::Serialize, serde::Deserialize)]
pub struct DependencyGraph {
    graph: DiGraph<Node, Edge>,
    node_indices: HashMap<NodeId, NodeIndex>,
    reverse_indices: HashMap<NodeIndex, NodeId>,
    runtime_traces: HashMap<NodeId, RuntimeMetrics>,
    #[serde(default)]
    pending_calls: Vec<(String, FunctionCall)>,
    #[serde(default)]
    pending_imports: Vec<(String, Import)>,
}

impl DependencyGraph {
    pub fn new() -> Self {
        Self {
            graph: DiGraph::new(),
            node_indices: HashMap::new(),
            reverse_indices: HashMap::new(),
            runtime_traces: HashMap::new(),
            pending_calls: Vec::new(),
            pending_imports: Vec::new(),
        }
    }

    pub fn get_all_nodes(&self) -> Vec<Node> {
        self.graph.node_weights().cloned().collect()
    }

    pub fn record_runtime_trace(&mut self, node_id: NodeId, executions: u64, latency_ms: f32) {
        let entry = self.runtime_traces.entry(node_id).or_insert(RuntimeMetrics {
            execution_count: 0,
            avg_latency_ms: 0.0,
            error_rate: 0.0,
            is_hotpath: false,
        });

        let previous_count = entry.execution_count;
        let new_count = previous_count.saturating_add(executions);
        if new_count > 0 {
            let weighted = (entry.avg_latency_ms as f64 * previous_count as f64)
                + (latency_ms as f64 * executions as f64);
            entry.avg_latency_ms = (weighted / new_count as f64) as f32;
        }
        entry.execution_count = new_count;
        entry.is_hotpath = entry.execution_count > 1000;
    }

    pub fn get_runtime_metrics(&self, node_id: &NodeId) -> Option<&RuntimeMetrics> {
        self.runtime_traces.get(node_id)
    }

    pub fn add_file(&mut self, analysis: &FileAnalysis) -> Result<()> {
        for node in &analysis.nodes {
            self.add_node(node.clone());
        }

        self.pending_imports.extend(
            analysis.imports.iter().cloned().map(|i| (analysis.path.clone(), i))
        );
        self.pending_calls.extend(
            analysis.calls.iter().cloned().map(|c| (analysis.path.clone(), c))
        );

        for rel in &analysis.type_relations {
            let from_id = NodeId(format!("{}::{}", analysis.path, rel.source_type));
            let to_id = NodeId(format!("{}::{}", analysis.path, rel.target_type));
            if let (Some(from_idx), Some(to_idx)) = (self.node_indices.get(&from_id), self.node_indices.get(&to_id)) {
                let edge_kind = match rel.kind {
                    TypeRelationKind::Extends => EdgeKind::Extends,
                    TypeRelationKind::Implements => EdgeKind::Implements,
                };
                self.add_edge_once(*from_idx, *to_idx, from_id, to_id, edge_kind, 2.0, HashMap::new());
            }
        }
        Ok(())
    }

    fn add_node(&mut self, node: Node) -> NodeIndex {
        if let Some(idx) = self.node_indices.get(&node.id) { return *idx; }
        let idx = self.graph.add_node(node.clone());
        self.node_indices.insert(node.id.clone(), idx);
        self.reverse_indices.insert(idx, node.id);
        idx
    }

    fn add_edge_once(
        &mut self,
        from_idx: NodeIndex,
        to_idx: NodeIndex,
        from: NodeId,
        to: NodeId,
        kind: EdgeKind,
        weight: f32,
        metadata: HashMap<String, String>,
    ) {
        let duplicate = self.graph.edges_connecting(from_idx, to_idx).any(|e| e.weight().kind == kind);
        if duplicate { return; }
        self.graph.add_edge(from_idx, to_idx, Edge {
            id: uuid::Uuid::new_v4(), from, to, kind, weight, metadata,
        });
    }

    fn normalize_lexical(path: &Path) -> String {
        let mut stack: Vec<String> = Vec::new();
        for c in path.components() {
            match c {
                Component::ParentDir => { stack.pop(); }
                Component::CurDir => {}
                Component::Normal(v) => stack.push(v.to_string_lossy().to_string()),
                Component::RootDir => stack.clear(),
                Component::Prefix(p) => stack.push(p.as_os_str().to_string_lossy().to_string()),
            }
        }
        stack.join("/")
    }

    fn file_nodes_by_path(&self) -> HashMap<String, NodeId> {
        self.graph.node_weights()
            .filter(|n| n.kind == NodeKind::File)
            .map(|n| (n.path.to_string_lossy().replace('\\', "/"), n.id.clone()))
            .collect()
    }

    fn import_candidates(from_file: &str, source: &str) -> Vec<String> {
        let exts = ["", ".ts", ".tsx", ".js", ".jsx", ".mjs", ".py", ".go", ".rs", ".java"];
        let mut bases = Vec::new();
        let normalized_from = from_file.replace('\\', "/");

        if source.starts_with('.') {
            let parent = Path::new(&normalized_from).parent().unwrap_or_else(|| Path::new(""));
            bases.push(Self::normalize_lexical(&parent.join(source)));
        } else if source.starts_with("crate::") || source.starts_with("self::") || source.starts_with("super::") {
            let rust_path = source
                .trim_start_matches("crate::")
                .trim_start_matches("self::")
                .trim_start_matches("super::")
                .replace("::", "/");
            let root = normalized_from.split("/src/").next().unwrap_or("");
            let prefix = if normalized_from.contains("/src/") { format!("{root}/src") } else { "src".to_string() };
            bases.push(format!("{}/{}", prefix.trim_end_matches('/'), rust_path));
        } else if source.contains('.') && !source.contains('/') {
            bases.push(source.replace('.', "/"));
            bases.push(format!("src/{}", source.replace('.', "/")));
        } else {
            bases.push(source.trim_start_matches('/').to_string());
            bases.push(format!("src/{}", source.trim_start_matches('/')));
        }

        let mut out = Vec::new();
        for base in bases {
            for ext in exts {
                out.push(format!("{base}{ext}"));
            }
            for index in ["index.ts", "index.tsx", "index.js", "index.py", "mod.rs"] {
                out.push(format!("{}/{}", base.trim_end_matches('/'), index));
            }
        }
        out
    }

    fn resolve_import_path(&self, from_file: &str, source: &str, files: &HashMap<String, NodeId>) -> Option<NodeId> {
        let normalized_files: HashMap<String, NodeId> = files.iter()
            .map(|(p, id)| (Self::normalize_lexical(Path::new(p)), id.clone()))
            .collect();
        for candidate in Self::import_candidates(from_file, source) {
            let c = Self::normalize_lexical(Path::new(&candidate));
            if let Some(id) = normalized_files.get(&c) { return Some(id.clone()); }
            let suffix = format!("/{c}");
            let matches: Vec<NodeId> = normalized_files.iter()
                .filter(|(p, _)| *p == &c || p.ends_with(&suffix))
                .map(|(_, id)| id.clone())
                .collect();
            if matches.len() == 1 { return Some(matches[0].clone()); }
        }
        None
    }

    fn function_node_candidates(&self, name: &str) -> Vec<NodeId> {
        self.graph.node_weights()
            .filter(|n| matches!(n.kind, NodeKind::Function | NodeKind::Method))
            .filter(|n| n.name == name || n.id.0.rsplit("::").next() == Some(name))
            .map(|n| n.id.clone())
            .collect()
    }

    pub fn build_call_graph(&mut self) -> Result<()> {
        let files = self.file_nodes_by_path();
        let pending_imports = std::mem::take(&mut self.pending_imports);

        for (from_file, import) in pending_imports {
            let from_id = NodeId(format!("{}::file", from_file));
            let Some(from_idx) = self.node_indices.get(&from_id).copied() else { continue; };
            if let Some(to_id) = self.resolve_import_path(&from_file, &import.source, &files) {
                if let Some(to_idx) = self.node_indices.get(&to_id).copied() {
                    let mut metadata = HashMap::new();
                    metadata.insert("import_source".into(), import.source.clone());
                    metadata.insert("resolution".into(), "semantic-path".into());
                    self.add_edge_once(from_idx, to_idx, from_id.clone(), to_id, EdgeKind::Import, 1.0, metadata);
                }
            }
        }

        let pending_calls = std::mem::take(&mut self.pending_calls);
        for (file, call) in pending_calls {
            let caller_id = NodeId(format!("{}::{}", file, call.caller_name));
            let Some(caller_idx) = self.node_indices.get(&caller_id).copied() else { continue; };

            let local_id = NodeId(format!("{}::{}", file, call.callee_name));
            let resolved = if self.node_indices.contains_key(&local_id) {
                Some((local_id, "same-file"))
            } else {
                let candidates = self.function_node_candidates(&call.callee_name);
                if candidates.len() == 1 { Some((candidates[0].clone(), "unique-symbol")) } else { None }
            };

            if let Some((callee_id, resolution)) = resolved {
                if let Some(callee_idx) = self.node_indices.get(&callee_id).copied() {
                    let mut metadata = HashMap::new();
                    metadata.insert("resolution".into(), resolution.into());
                    metadata.insert("call_line".into(), call.line.to_string());
                    metadata.insert("call_column".into(), call.column.to_string());
                    self.add_edge_once(caller_idx, callee_idx, caller_id.clone(), callee_id, EdgeKind::Calls, 1.5, metadata);
                }
            }
        }
        Ok(())
    }

    pub fn build_type_graph(&mut self) -> Result<()> {
        Ok(())
    }

    pub fn get_callers(&self, node_id: &NodeId) -> Vec<NodeId> {
        let mut callers = Vec::new();
        if let Some(idx) = self.node_indices.get(node_id) {
            for n_idx in self.graph.neighbors_directed(*idx, petgraph::Direction::Incoming) {
                if let Some(n_id) = self.reverse_indices.get(&n_idx) { callers.push(n_id.clone()); }
            }
        }
        callers
    }

    pub fn get_callees(&self, node_id: &NodeId) -> Vec<NodeId> {
        let mut callees = Vec::new();
        if let Some(idx) = self.node_indices.get(node_id) {
            for n_idx in self.graph.neighbors_directed(*idx, petgraph::Direction::Outgoing) {
                if let Some(n_id) = self.reverse_indices.get(&n_idx) { callees.push(n_id.clone()); }
            }
        }
        callees
    }

    pub fn find_affected_nodes(&self, file: &str, line: u32) -> Result<HashSet<NodeId>> {
        let normalized = file.replace('\\', "/");
        let mut candidates: Vec<(&NodeId, &Node)> = self.node_indices.iter()
            .filter_map(|(id, idx)| {
                let node = &self.graph[*idx];
                let path = node.path.to_string_lossy().replace('\\', "/");
                if path == normalized || path.ends_with(&format!("/{normalized}")) { Some((id, node)) } else { None }
            })
            .collect();

        let mut affected = HashSet::new();
        if candidates.is_empty() { return Ok(affected); }
        candidates.sort_by_key(|(_, node)| node.line);
        if let Some((id, _)) = candidates.iter().rev().find(|(_, node)| node.line <= line && node.kind != NodeKind::File) {
            affected.insert((*id).clone());
            return Ok(affected);
        }
        if let Some((id, _)) = candidates.iter().find(|(_, n)| n.kind == NodeKind::File) {
            affected.insert((*id).clone());
        } else if let Some((id, _)) = candidates.first() {
            affected.insert((*id).clone());
        }
        Ok(affected)
    }

    pub fn calculate_impact(&self, affected: &HashSet<NodeId>, _change_type: ChangeType) -> Result<ImpactAnalysis> {
        let mut direct = Vec::new();
        let mut indirect = Vec::new();
        let mut visited: HashSet<NodeId> = affected.iter().cloned().collect();
        let mut queue: VecDeque<(NodeIndex, usize)> = VecDeque::new();
        let mut max_depth = 0usize;

        for node_id in affected {
            if let Some(idx) = self.node_indices.get(node_id) { queue.push_back((*idx, 0)); }
        }

        while let Some((current_idx, depth)) = queue.pop_front() {
            for neighbor_idx in self.graph.neighbors_directed(current_idx, petgraph::Direction::Incoming) {
                let Some(neighbor_id) = self.reverse_indices.get(&neighbor_idx).cloned() else { continue; };
                if !visited.insert(neighbor_id.clone()) { continue; }
                let next_depth = depth + 1;
                max_depth = max_depth.max(next_depth);
                let confidence = (0.95_f32 * 0.85_f32.powi((next_depth.saturating_sub(1)) as i32)).max(0.25);
                let impacted = ImpactedNode {
                    node: neighbor_id,
                    impact_kind: if next_depth == 1 { ImpactKind::CompileBreak } else { ImpactKind::Behavioral },
                    confidence,
                    path: self.graph[neighbor_idx].path.clone(),
                    line: self.graph[neighbor_idx].line,
                };
                if next_depth == 1 { direct.push(impacted); } else { indirect.push(impacted); }
                queue.push_back((neighbor_idx, next_depth));
            }
        }

        let breadth = direct.len() as f32 * 2.0 + indirect.len() as f32;
        let depth_factor = (max_depth as f32 * 0.08).min(0.24);
        let risk_score = ((breadth / 20.0) + depth_factor).min(1.0);
        let effort = if risk_score >= 0.70 { "High" } else if risk_score >= 0.30 { "Medium" } else { "Low" }.to_string();
        Ok(ImpactAnalysis { direct_impacts: direct, indirect_impacts: indirect, risk_score, estimated_effort: effort })
    }

    pub fn node_count(&self) -> usize { self.graph.node_count() }
    pub fn edge_count(&self) -> usize { self.graph.edge_count() }
    pub fn nodes(&self) -> Vec<&Node> { self.graph.node_weights().collect() }
    pub fn edges(&self) -> Vec<&Edge> { self.graph.edge_weights().collect() }

    pub fn find_node_by_path(&self, path: &str) -> Option<&Node> {
        for idx in self.graph.node_indices() {
            let node = &self.graph[idx];
            if node.path.to_string_lossy() == path { return Some(node); }
        }
        None
    }

    pub fn get_dependencies(&self, node_id: &NodeId) -> Result<Vec<NodeId>> {
        let mut deps = Vec::new();
        if let Some(idx) = self.node_indices.get(node_id) {
            for neighbor_idx in self.graph.neighbors_directed(*idx, petgraph::Direction::Outgoing) {
                if let Some(neighbor_id) = self.reverse_indices.get(&neighbor_idx) { deps.push(neighbor_id.clone()); }
            }
        }
        Ok(deps)
    }

    pub fn extract_subgraph(&self, node_ids: &HashSet<NodeId>) -> Result<DependencyGraph> {
        let mut subgraph = DependencyGraph::new();
        for node_id in node_ids {
            if let Some(idx) = self.node_indices.get(node_id) { subgraph.add_node(self.graph[*idx].clone()); }
        }
        for edge in self.graph.edge_indices() {
            if let Some((from_idx, to_idx)) = self.graph.edge_endpoints(edge) {
                let from_id = self.reverse_indices.get(&from_idx);
                let to_id = self.reverse_indices.get(&to_idx);
                if let (Some(from), Some(to)) = (from_id, to_id) {
                    if node_ids.contains(from) && node_ids.contains(to) {
                        if let (Some(from_new), Some(to_new)) = (subgraph.node_indices.get(from), subgraph.node_indices.get(to)) {
                            subgraph.graph.add_edge(*from_new, *to_new, self.graph[edge].clone());
                        }
                    }
                }
            }
        }
        Ok(subgraph)
    }

    pub fn incoming_degree(&self, node_id: &NodeId) -> Result<usize> {
        Ok(self.node_indices.get(node_id).map(|idx| self.graph.neighbors_directed(*idx, petgraph::Direction::Incoming).count()).unwrap_or(0))
    }

    pub fn outgoing_degree(&self, node_id: &NodeId) -> Result<usize> {
        Ok(self.node_indices.get(node_id).map(|idx| self.graph.neighbors_directed(*idx, petgraph::Direction::Outgoing).count()).unwrap_or(0))
    }

    pub fn find_cycles(&self) -> Result<Vec<Vec<NodeId>>> {
        let mut cycles = Vec::new();
        for scc in petgraph::algo::kosaraju_scc(&self.graph) {
            if scc.len() > 1 {
                cycles.push(scc.iter().filter_map(|idx| self.reverse_indices.get(idx).cloned()).collect());
            }
        }
        Ok(cycles)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn node(path: &str, name: &str, kind: NodeKind, line: u32) -> Node {
        Node {
            id: NodeId(format!("{}::{}", path, if kind == NodeKind::File { "file" } else { name })),
            kind,
            name: name.to_string(),
            path: PathBuf::from(path),
            line,
            column: 0,
            exports: vec![],
            imports: vec![],
            metadata: HashMap::new(),
        }
    }

    fn analysis(path: &str, nodes: Vec<Node>, imports: Vec<Import>, calls: Vec<FunctionCall>) -> FileAnalysis {
        FileAnalysis { path: path.to_string(), nodes, imports, exports: vec![], calls, type_relations: vec![] }
    }

    #[test]
    fn resolves_relative_imports_after_all_files_are_loaded() {
        let mut graph = DependencyGraph::new();
        graph.add_file(&analysis(
            "src/a.ts",
            vec![node("src/a.ts", "a.ts", NodeKind::File, 1)],
            vec![Import { source: "./b".into(), symbols: vec![], kind: ImportKind::Named }],
            vec![],
        )).unwrap();
        graph.add_file(&analysis(
            "src/b.ts",
            vec![node("src/b.ts", "b.ts", NodeKind::File, 1)],
            vec![], vec![],
        )).unwrap();

        graph.build_call_graph().unwrap();
        let deps = graph.get_dependencies(&NodeId("src/a.ts::file".into())).unwrap();
        assert!(deps.contains(&NodeId("src/b.ts::file".into())));
    }

    #[test]
    fn resolves_unique_cross_file_function_calls() {
        let mut graph = DependencyGraph::new();
        graph.add_file(&analysis(
            "src/a.ts",
            vec![
                node("src/a.ts", "a.ts", NodeKind::File, 1),
                node("src/a.ts", "start", NodeKind::Function, 3),
            ],
            vec![],
            vec![FunctionCall { caller_name: "start".into(), callee_name: "work".into(), line: 4, column: 2 }],
        )).unwrap();
        graph.add_file(&analysis(
            "src/b.ts",
            vec![
                node("src/b.ts", "b.ts", NodeKind::File, 1),
                node("src/b.ts", "work", NodeKind::Function, 6),
            ],
            vec![], vec![],
        )).unwrap();

        graph.build_call_graph().unwrap();
        let callees = graph.get_callees(&NodeId("src/a.ts::start".into()));
        assert_eq!(callees, vec![NodeId("src/b.ts::work".into())]);
    }

    #[test]
    fn line_targeting_selects_nearest_declaration_not_every_file_symbol() {
        let mut graph = DependencyGraph::new();
        graph.add_file(&analysis(
            "src/a.ts",
            vec![
                node("src/a.ts", "a.ts", NodeKind::File, 1),
                node("src/a.ts", "first", NodeKind::Function, 5),
                node("src/a.ts", "second", NodeKind::Function, 30),
            ], vec![], vec![],
        )).unwrap();
        let affected = graph.find_affected_nodes("src/a.ts", 35).unwrap();
        assert_eq!(affected.len(), 1);
        assert!(affected.contains(&NodeId("src/a.ts::second".into())));
    }

    #[test]
    fn runtime_average_is_weighted_by_observation_count() {
        let mut graph = DependencyGraph::new();
        let id = NodeId("src/a.ts::work".into());
        graph.record_runtime_trace(id.clone(), 100, 10.0);
        graph.record_runtime_trace(id.clone(), 1, 1000.0);
        let m = graph.get_runtime_metrics(&id).unwrap();
        assert_eq!(m.execution_count, 101);
        assert!((m.avg_latency_ms - 19.80198).abs() < 0.01);
    }
}
