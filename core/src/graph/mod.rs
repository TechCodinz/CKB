//! Dependency graph construction and querying

use std::collections::{HashMap, HashSet, VecDeque};
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
}

impl DependencyGraph {
    pub fn new() -> Self {
        Self {
            graph: DiGraph::new(),
            node_indices: HashMap::new(),
            reverse_indices: HashMap::new(),
            runtime_traces: HashMap::new(),
        }
    }

    pub fn get_all_nodes(&self) -> Vec<Node> {
        self.graph.node_weights().cloned().collect()
    }

    /// Record runtime telemetry using a count-weighted latency average.
    /// The previous implementation averaged averages 50/50 regardless of how
    /// many observations each batch represented, which distorted live data.
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

        for import in &analysis.imports {
            let from_id = NodeId(format!("{}::file", analysis.path));
            let to_id = NodeId(format!("{}::file", import.source));

            if let (Some(from_idx), Some(to_idx)) = (self.node_indices.get(&from_id), self.node_indices.get(&to_id)) {
                self.graph.add_edge(*from_idx, *to_idx, Edge {
                    id: uuid::Uuid::new_v4(),
                    from: from_id.clone(),
                    to: to_id.clone(),
                    kind: EdgeKind::Import,
                    weight: 1.0,
                    metadata: Default::default(),
                });
            }
        }

        for call in &analysis.calls {
            let from_id = NodeId(format!("{}::{}", analysis.path, call.caller_name));
            let to_id = NodeId(format!("{}::{}", analysis.path, call.callee_name));
            if let (Some(from_idx), Some(to_idx)) = (self.node_indices.get(&from_id), self.node_indices.get(&to_id)) {
                self.graph.add_edge(*from_idx, *to_idx, Edge {
                    id: uuid::Uuid::new_v4(),
                    from: from_id.clone(),
                    to: to_id.clone(),
                    kind: EdgeKind::Calls,
                    weight: 1.5,
                    metadata: Default::default(),
                });
            }
        }

        for rel in &analysis.type_relations {
            let from_id = NodeId(format!("{}::{}", analysis.path, rel.source_type));
            let to_id = NodeId(format!("{}::{}", analysis.path, rel.target_type));
            if let (Some(from_idx), Some(to_idx)) = (self.node_indices.get(&from_id), self.node_indices.get(&to_id)) {
                let edge_kind = match rel.kind {
                    crate::types::TypeRelationKind::Extends => EdgeKind::Extends,
                    crate::types::TypeRelationKind::Implements => EdgeKind::Implements,
                };
                self.graph.add_edge(*from_idx, *to_idx, Edge {
                    id: uuid::Uuid::new_v4(),
                    from: from_id.clone(),
                    to: to_id.clone(),
                    kind: edge_kind,
                    weight: 2.0,
                    metadata: Default::default(),
                });
            }
        }

        Ok(())
    }

    fn add_node(&mut self, node: Node) -> NodeIndex {
        if let Some(idx) = self.node_indices.get(&node.id) {
            return *idx;
        }

        let idx = self.graph.add_node(node.clone());
        self.node_indices.insert(node.id.clone(), idx);
        self.reverse_indices.insert(idx, node.id);
        idx
    }

    pub fn build_call_graph(&mut self) -> Result<()> {
        let node_list: Vec<(NodeId, String)> = self.graph.node_weights()
            .map(|n| (n.id.clone(), n.name.clone()))
            .collect();

        for (from_id, name) in &node_list {
            if let Some(from_idx) = self.node_indices.get(from_id) {
                for (to_id, target_name) in &node_list {
                    if from_id != to_id && name.contains(target_name) {
                        if let Some(to_idx) = self.node_indices.get(to_id) {
                            self.graph.add_edge(*from_idx, *to_idx, Edge {
                                id: uuid::Uuid::new_v4(),
                                from: from_id.clone(),
                                to: to_id.clone(),
                                kind: EdgeKind::Calls,
                                weight: 1.2,
                                metadata: Default::default(),
                            });
                        }
                    }
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
                if let Some(n_id) = self.reverse_indices.get(&n_idx) {
                    callers.push(n_id.clone());
                }
            }
        }
        callers
    }

    pub fn get_callees(&self, node_id: &NodeId) -> Vec<NodeId> {
        let mut callees = Vec::new();
        if let Some(idx) = self.node_indices.get(node_id) {
            for n_idx in self.graph.neighbors_directed(*idx, petgraph::Direction::Outgoing) {
                if let Some(n_id) = self.reverse_indices.get(&n_idx) {
                    callees.push(n_id.clone());
                }
            }
        }
        callees
    }

    /// Resolve a line to the most specific declaration CKB currently knows.
    /// Parsers store declaration start lines; until end-spans are available,
    /// the declaration with the greatest start line <= requested line is a
    /// substantially more precise target than marking every node in the file.
    pub fn find_affected_nodes(&self, file: &str, line: u32) -> Result<HashSet<NodeId>> {
        let normalized = file.replace('\\', "/");
        let mut candidates: Vec<(&NodeId, &Node)> = self.node_indices.iter()
            .filter_map(|(id, idx)| {
                let node = &self.graph[*idx];
                let path = node.path.to_string_lossy().replace('\\', "/");
                if path == normalized { Some((id, node)) } else { None }
            })
            .collect();

        let mut affected = HashSet::new();
        if candidates.is_empty() { return Ok(affected); }

        candidates.sort_by_key(|(_, node)| node.line);
        if let Some((id, _)) = candidates.iter().rev().find(|(_, node)| node.line <= line && node.kind != NodeKind::File) {
            affected.insert((*id).clone());
            return Ok(affected);
        }

        // Fall back to the file node, or the earliest declaration if a parser
        // did not emit a dedicated file node.
        if let Some((id, _)) = candidates.iter().find(|(_, n)| n.kind == NodeKind::File) {
            affected.insert((*id).clone());
        } else if let Some((id, _)) = candidates.first() {
            affected.insert((*id).clone());
        }
        Ok(affected)
    }

    /// Traverse ALL incoming dependents using BFS. Direct impacts are depth 1;
    /// every deeper reachable dependent is an indirect impact. Confidence
    /// decays with graph distance rather than using a fixed two-hop ceiling.
    pub fn calculate_impact(&self, affected: &HashSet<NodeId>, _change_type: ChangeType) -> Result<ImpactAnalysis> {
        let mut direct = Vec::new();
        let mut indirect = Vec::new();
        let mut visited: HashSet<NodeId> = affected.iter().cloned().collect();
        let mut queue: VecDeque<(NodeIndex, usize)> = VecDeque::new();
        let mut max_depth = 0usize;

        for node_id in affected {
            if let Some(idx) = self.node_indices.get(node_id) {
                queue.push_back((*idx, 0));
            }
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

        Ok(ImpactAnalysis {
            direct_impacts: direct,
            indirect_impacts: indirect,
            risk_score,
            estimated_effort: effort,
        })
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
                if let Some(neighbor_id) = self.reverse_indices.get(&neighbor_idx) {
                    deps.push(neighbor_id.clone());
                }
            }
        }
        Ok(deps)
    }

    pub fn extract_subgraph(&self, node_ids: &HashSet<NodeId>) -> Result<DependencyGraph> {
        let mut subgraph = DependencyGraph::new();
        for node_id in node_ids {
            if let Some(idx) = self.node_indices.get(node_id) {
                subgraph.add_node(self.graph[*idx].clone());
            }
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
        Ok(self.node_indices.get(node_id)
            .map(|idx| self.graph.neighbors_directed(*idx, petgraph::Direction::Incoming).count())
            .unwrap_or(0))
    }

    pub fn outgoing_degree(&self, node_id: &NodeId) -> Result<usize> {
        Ok(self.node_indices.get(node_id)
            .map(|idx| self.graph.neighbors_directed(*idx, petgraph::Direction::Outgoing).count())
            .unwrap_or(0))
    }

    pub fn find_cycles(&self) -> Result<Vec<Vec<NodeId>>> {
        let mut cycles = Vec::new();
        let sccs = petgraph::algo::kosaraju_scc(&self.graph);
        for scc in sccs {
            if scc.len() > 1 {
                cycles.push(scc.iter().filter_map(|idx| self.reverse_indices.get(idx).cloned()).collect());
            }
        }
        Ok(cycles)
    }
}
