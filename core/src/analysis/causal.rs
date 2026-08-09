//! Evidence-backed causal architecture reasoning.
//!
//! CKB uses this module to answer questions such as "why does A depend on B?",
//! "what path carries this change to that service?", and "which callers can be
//! affected upstream?" without asking an LLM to guess from filenames. Paths
//! are graph facts; runtime evidence is attached only when it actually exists.

use crate::graph::DependencyGraph;
use crate::types::{EdgeKind, NodeId};
use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet, VecDeque};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CausalEvidence {
    pub source: String,
    pub reference: String,
    pub kind: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CausalPathStep {
    pub from: String,
    pub to: String,
    pub relationship: String,
    pub confidence: f32,
    pub source_path: Option<String>,
    pub source_line: Option<u32>,
    pub runtime_observed_at_from: bool,
    pub runtime_observed_at_to: bool,
    pub evidence: Vec<CausalEvidence>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CausalPathReport {
    pub found: bool,
    pub source: String,
    pub target: String,
    pub hops: usize,
    pub path_confidence: f32,
    pub steps: Vec<CausalPathStep>,
    pub explanation: String,
    pub evidence_policy: String,
    pub synthetic: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FailureConeNode {
    pub id: String,
    pub depth: usize,
    pub path: String,
    pub line: u32,
    pub runtime_observed: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FailureConeReport {
    pub root: String,
    pub max_depth: usize,
    pub affected: Vec<FailureConeNode>,
    pub total_affected: usize,
    pub evidence_policy: String,
    pub synthetic: bool,
}

pub struct CausalArchitectureEngine;

impl CausalArchitectureEngine {
    fn kind<T: std::fmt::Debug>(value: T) -> String {
        format!("{:?}", value).to_ascii_lowercase()
    }

    fn edge_confidence(kind: EdgeKind, metadata: &HashMap<String, String>) -> f32 {
        let base: f32 = match kind {
            EdgeKind::Calls => 0.98,
            EdgeKind::Import => 0.96,
            EdgeKind::Extends | EdgeKind::Implements => 0.99,
            EdgeKind::Instantiates => 0.98,
            EdgeKind::Returns | EdgeKind::Parameter | EdgeKind::Property => 0.94,
        };
        let resolution_bonus: f32 = match metadata.get("resolution").map(String::as_str) {
            Some("same-file") | Some("semantic-path") => 0.01,
            Some("unique-symbol") => 0.0,
            Some(_) => -0.02,
            None => 0.0,
        };
        (base + resolution_bonus).clamp(0.50_f32, 1.0_f32)
    }

    /// Directed shortest path through proven architectural relationships.
    /// This deliberately does not invent a connection when the graph has none.
    pub fn shortest_path(
        graph: &DependencyGraph,
        source: &NodeId,
        target: &NodeId,
        max_depth: usize,
    ) -> Result<CausalPathReport> {
        let max_depth = max_depth.clamp(1, 32);
        if source == target {
            return Ok(CausalPathReport {
                found: true,
                source: source.0.clone(),
                target: target.0.clone(),
                hops: 0,
                path_confidence: 1.0,
                steps: vec![],
                explanation: "Source and target are the same architecture node.".into(),
                evidence_policy: "static-paths-are-graph-facts; runtime-is-explicit".into(),
                synthetic: false,
            });
        }

        let mut outgoing: HashMap<NodeId, Vec<&crate::types::Edge>> = HashMap::new();
        for edge in graph.edges() {
            outgoing.entry(edge.from.clone()).or_default().push(edge);
        }

        let mut visited = HashSet::new();
        visited.insert(source.clone());
        let mut queue: VecDeque<(NodeId, usize)> = VecDeque::from([(source.clone(), 0)]);
        let mut parent: HashMap<NodeId, (NodeId, uuid::Uuid)> = HashMap::new();
        let mut found = false;

        while let Some((current, depth)) = queue.pop_front() {
            if depth >= max_depth { continue; }
            for edge in outgoing.get(&current).into_iter().flatten() {
                let next = edge.to.clone();
                if !visited.insert(next.clone()) { continue; }
                parent.insert(next.clone(), (current.clone(), edge.id));
                if next == *target {
                    found = true;
                    queue.clear();
                    break;
                }
                queue.push_back((next, depth + 1));
            }
        }

        if !found {
            return Ok(CausalPathReport {
                found: false,
                source: source.0.clone(),
                target: target.0.clone(),
                hops: 0,
                path_confidence: 0.0,
                steps: vec![],
                explanation: format!("No directed architecture path from `{}` to `{}` was found within {} hops.", source.0, target.0, max_depth),
                evidence_policy: "absence-of-path-is-not-proof-of-runtime-impossibility".into(),
                synthetic: false,
            });
        }

        let edges_by_id: HashMap<uuid::Uuid, &crate::types::Edge> = graph.edges().into_iter().map(|e| (e.id, e)).collect();
        let nodes_by_id: HashMap<NodeId, &crate::types::Node> = graph.nodes().into_iter().map(|n| (n.id.clone(), n)).collect();
        let mut edge_ids = Vec::new();
        let mut cursor = target.clone();
        while cursor != *source {
            let Some((prev, edge_id)) = parent.get(&cursor).cloned() else { break; };
            edge_ids.push(edge_id);
            cursor = prev;
        }
        edge_ids.reverse();

        let mut steps = Vec::new();
        let mut confidence = 1.0_f32;
        for edge_id in edge_ids {
            let Some(edge) = edges_by_id.get(&edge_id).copied() else { continue; };
            let edge_conf = Self::edge_confidence(edge.kind, &edge.metadata);
            confidence *= edge_conf;
            let from_node = nodes_by_id.get(&edge.from).copied();
            let mut evidence = vec![CausalEvidence {
                source: "ckb-architecture-graph".into(),
                reference: format!("{} -> {}", edge.from.0, edge.to.0),
                kind: "static".into(),
            }];
            if let Some(resolution) = edge.metadata.get("resolution") {
                evidence.push(CausalEvidence { source: "semantic-resolution".into(), reference: resolution.clone(), kind: "static".into() });
            }
            if let Some(line) = edge.metadata.get("call_line") {
                evidence.push(CausalEvidence { source: "tree-sitter-callsite".into(), reference: line.clone(), kind: "static".into() });
            }
            let from_runtime = graph.get_runtime_metrics(&edge.from).is_some();
            let to_runtime = graph.get_runtime_metrics(&edge.to).is_some();
            if from_runtime || to_runtime {
                evidence.push(CausalEvidence {
                    source: "runtime-telemetry".into(),
                    reference: format!("from_observed={} to_observed={}", from_runtime, to_runtime),
                    kind: "runtime".into(),
                });
            }
            steps.push(CausalPathStep {
                from: edge.from.0.clone(),
                to: edge.to.0.clone(),
                relationship: Self::kind(edge.kind),
                confidence: edge_conf,
                source_path: from_node.map(|n| n.path.to_string_lossy().to_string()),
                source_line: edge.metadata.get("call_line").and_then(|v| v.parse::<u32>().ok()).or_else(|| from_node.map(|n| n.line)),
                runtime_observed_at_from: from_runtime,
                runtime_observed_at_to: to_runtime,
                evidence,
            });
        }

        let explanation = if steps.is_empty() {
            "A graph path was resolved but no edge evidence could be reconstructed.".to_string()
        } else {
            format!(
                "CKB found a {}-hop directed architecture path from `{}` to `{}`. This proves a static relationship path; runtime execution is claimed only on steps carrying runtime evidence.",
                steps.len(), source.0, target.0
            )
        };

        Ok(CausalPathReport {
            found: true,
            source: source.0.clone(),
            target: target.0.clone(),
            hops: steps.len(),
            path_confidence: confidence.clamp(0.0, 1.0),
            steps,
            explanation,
            evidence_policy: "static-paths-are-graph-facts; runtime-is-explicit".into(),
            synthetic: false,
        })
    }

    /// Upstream change/failure cone: all transitive dependents of a node.
    pub fn failure_cone(graph: &DependencyGraph, root: &NodeId, max_depth: usize) -> Result<FailureConeReport> {
        let max_depth = max_depth.clamp(1, 32);
        let nodes_by_id: HashMap<NodeId, &crate::types::Node> = graph.nodes().into_iter().map(|n| (n.id.clone(), n)).collect();
        let mut incoming: HashMap<NodeId, Vec<NodeId>> = HashMap::new();
        for edge in graph.edges() {
            incoming.entry(edge.to.clone()).or_default().push(edge.from.clone());
        }

        let mut visited = HashSet::from([root.clone()]);
        let mut queue: VecDeque<(NodeId, usize)> = VecDeque::from([(root.clone(), 0)]);
        let mut affected = Vec::new();
        while let Some((current, depth)) = queue.pop_front() {
            if depth >= max_depth { continue; }
            for dependent in incoming.get(&current).into_iter().flatten() {
                if !visited.insert(dependent.clone()) { continue; }
                let next_depth = depth + 1;
                if let Some(node) = nodes_by_id.get(dependent).copied() {
                    affected.push(FailureConeNode {
                        id: dependent.0.clone(),
                        depth: next_depth,
                        path: node.path.to_string_lossy().to_string(),
                        line: node.line,
                        runtime_observed: graph.get_runtime_metrics(dependent).is_some(),
                    });
                }
                queue.push_back((dependent.clone(), next_depth));
            }
        }
        affected.sort_by(|a, b| a.depth.cmp(&b.depth).then_with(|| a.id.cmp(&b.id)));
        Ok(FailureConeReport {
            root: root.0.clone(),
            max_depth,
            total_affected: affected.len(),
            affected,
            evidence_policy: "transitive-dependents-from-current-graph; not-a-runtime-failure-claim".into(),
            synthetic: false,
        })
    }
}
