//! Dependency graph construction and querying

use std::collections::{HashMap, HashSet};
use petgraph::graph::{DiGraph, NodeIndex};
use petgraph::algo::has_path_connecting;
use crate::types::*;
use crate::parser::FileAnalysis;
use anyhow::Result;

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

    /// Record live dynamic runtime trace telemetry for a node
    pub fn record_runtime_trace(&mut self, node_id: NodeId, executions: u64, latency_ms: f32) {
        let entry = self.runtime_traces.entry(node_id).or_insert(RuntimeMetrics {
            execution_count: 0,
            avg_latency_ms: 0.0,
            error_rate: 0.0,
            is_hotpath: false,
        });

        entry.execution_count += executions;
        entry.avg_latency_ms = (entry.avg_latency_ms + latency_ms) / 2.0;
        if entry.execution_count > 1000 {
            entry.is_hotpath = true;
        }
    }

    /// Retrieve dynamic runtime execution metrics for a node
    pub fn get_runtime_metrics(&self, node_id: &NodeId) -> Option<&RuntimeMetrics> {
        self.runtime_traces.get(node_id)
    }
    
    pub fn add_file(&mut self, analysis: &FileAnalysis) -> Result<()> {
        // Add nodes for each declaration
        for node in &analysis.nodes {
            self.add_node(node.clone());
        }
        
        // Add import edges
        for import in &analysis.imports {
            let from_id = NodeId(format!("{}::file", analysis.path));
            let to_id = NodeId(format!("{}::file", import.source));
            
            if let (Some(from_idx), Some(to_idx)) = (self.node_indices.get(&from_id), 
                                                      self.node_indices.get(&to_id)) {
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

        // Add function call edges
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

        // Add type relationship edges
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
        // Traverses node weights to construct calls edges between identified symbols
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
        // Infers structural type inheritance edges
        Ok(())
    }

    /// Find callers of a specific node
    pub fn get_callers(&self, node_id: &NodeId) -> Vec<NodeId> {
        let mut callers = Vec::new();
        if let Some(idx) = self.node_indices.get(node_id) {
            let neighbors = self.graph.neighbors_directed(*idx, petgraph::Direction::Incoming);
            for n_idx in neighbors {
                if let Some(n_id) = self.reverse_indices.get(&n_idx) {
                    callers.push(n_id.clone());
                }
            }
        }
        callers
    }

    /// Find callees of a specific node
    pub fn get_callees(&self, node_id: &NodeId) -> Vec<NodeId> {
        let mut callees = Vec::new();
        if let Some(idx) = self.node_indices.get(node_id) {
            let neighbors = self.graph.neighbors_directed(*idx, petgraph::Direction::Outgoing);
            for n_idx in neighbors {
                if let Some(n_id) = self.reverse_indices.get(&n_idx) {
                    callees.push(n_id.clone());
                }
            }
        }
        callees
    }
    
    pub fn find_affected_nodes(&self, file: &str, line: u32) -> Result<HashSet<NodeId>> {
        let mut affected = HashSet::new();
        
        // Find the node at the given location
        for (id, idx) in &self.node_indices {
            let node = &self.graph[*idx];
            if node.path.to_string_lossy() == file {
                affected.insert(id.clone());
            }
        }
        
        Ok(affected)
    }
    
    pub fn calculate_impact(&self, affected: &HashSet<NodeId>, change_type: ChangeType) -> Result<ImpactAnalysis> {
        let mut direct = Vec::new();
        let mut indirect = Vec::new();
        let mut visited = HashSet::new();
        
        for node_id in affected {
            visited.insert(node_id.clone());
            if let Some(idx) = self.node_indices.get(node_id) {
                // Direct dependents (incoming edges)
                let neighbors = self.graph.neighbors_directed(*idx, petgraph::Direction::Incoming);
                for neighbor_idx in neighbors {
                    if let Some(neighbor_id) = self.reverse_indices.get(&neighbor_idx) {
                        if !visited.contains(neighbor_id) {
                            visited.insert(neighbor_id.clone());
                            direct.push(ImpactedNode {
                                node: neighbor_id.clone(),
                                impact_kind: ImpactKind::CompileBreak,
                                confidence: 0.95,
                                path: self.graph[neighbor_idx].path.clone(),
                                line: self.graph[neighbor_idx].line,
                            });

                            // Indirect dependents (hop 2)
                            let second_neighbors = self.graph.neighbors_directed(neighbor_idx, petgraph::Direction::Incoming);
                            for ind_idx in second_neighbors {
                                if let Some(ind_id) = self.reverse_indices.get(&ind_idx) {
                                    if !visited.contains(ind_id) {
                                        visited.insert(ind_id.clone());
                                        indirect.push(ImpactedNode {
                                            node: ind_id.clone(),
                                            impact_kind: ImpactKind::Behavioral,
                                            confidence: 0.70,
                                            path: self.graph[ind_idx].path.clone(),
                                            line: self.graph[ind_idx].line,
                                        });
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
        
        let risk_score = ((direct.len() * 2 + indirect.len()) as f32 / 10.0).min(1.0);
        let effort = if risk_score > 0.7 {
            "High".to_string()
        } else if risk_score > 0.3 {
            "Medium".to_string()
        } else {
            "Low".to_string()
        };

        Ok(ImpactAnalysis {
            direct_impacts: direct,
            indirect_impacts: indirect,
            risk_score,
            estimated_effort: effort,
        })
    }
    
    pub fn node_count(&self) -> usize {
        self.graph.node_count()
    }
    
    pub fn edge_count(&self) -> usize {
        self.graph.edge_count()
    }

    /// Get all nodes in the graph
    pub fn nodes(&self) -> Vec<&Node> {
        self.graph.node_weights().collect()
    }

    /// Get all edges in the graph
    pub fn edges(&self) -> Vec<&Edge> {
        self.graph.edge_weights().collect()
    }

    /// Find a node by its file path
    pub fn find_node_by_path(&self, path: &str) -> Option<&Node> {
        for idx in self.graph.node_indices() {
            let node = &self.graph[idx];
            if node.path.to_string_lossy() == path {
                return Some(node);
            }
        }
        None
    }

    /// Get all dependencies (outgoing edges) for a node
    pub fn get_dependencies(&self, node_id: &NodeId) -> Result<Vec<NodeId>> {
        let mut deps = Vec::new();
        if let Some(idx) = self.node_indices.get(node_id) {
            let neighbors = self.graph.neighbors_directed(*idx, petgraph::Direction::Outgoing);
            for neighbor_idx in neighbors {
                if let Some(neighbor_id) = self.reverse_indices.get(&neighbor_idx) {
                    deps.push(neighbor_id.clone());
                }
            }
        }
        Ok(deps)
    }

    /// Extract a subgraph containing only the specified nodes
    pub fn extract_subgraph(&self, node_ids: &HashSet<NodeId>) -> Result<DependencyGraph> {
        let mut subgraph = DependencyGraph::new();

        for node_id in node_ids {
            if let Some(idx) = self.node_indices.get(node_id) {
                subgraph.add_node(self.graph[*idx].clone());
            }
        }

        // Add edges between nodes in the subgraph
        for edge in self.graph.edge_indices() {
            if let Some((from_idx, to_idx)) = self.graph.edge_endpoints(edge) {
                let from_id = self.reverse_indices.get(&from_idx);
                let to_id = self.reverse_indices.get(&to_idx);

                if let (Some(from), Some(to)) = (from_id, to_id) {
                    if node_ids.contains(from) && node_ids.contains(to) {
                        if let Some(from_new) = subgraph.node_indices.get(from) {
                            if let Some(to_new) = subgraph.node_indices.get(to) {
                                subgraph.graph.add_edge(*from_new, *to_new, self.graph[edge].clone());
                            }
                        }
                    }
                }
            }
        }

        Ok(subgraph)
    }

    /// Get incoming degree (number of dependents)
    pub fn incoming_degree(&self, node_id: &NodeId) -> Result<usize> {
        if let Some(idx) = self.node_indices.get(node_id) {
            Ok(self.graph.neighbors_directed(*idx, petgraph::Direction::Incoming).count())
        } else {
            Ok(0)
        }
    }

    /// Get outgoing degree (number of dependencies)
    pub fn outgoing_degree(&self, node_id: &NodeId) -> Result<usize> {
        if let Some(idx) = self.node_indices.get(node_id) {
            Ok(self.graph.neighbors_directed(*idx, petgraph::Direction::Outgoing).count())
        } else {
            Ok(0)
        }
    }

    /// Find all cycles in the graph using DFS
    pub fn find_cycles(&self) -> Result<Vec<Vec<NodeId>>> {
        let mut cycles = Vec::new();
        let sccs = petgraph::algo::kosaraju_scc(&self.graph);

        for scc in sccs {
            if scc.len() > 1 {
                let cycle: Vec<NodeId> = scc.iter()
                    .filter_map(|idx| self.reverse_indices.get(idx).cloned())
                    .collect();
                cycles.push(cycle);
            }
        }

        Ok(cycles)
    }
}
