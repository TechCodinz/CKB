//! Evidence-backed architecture memory retrieval and Code DNA scoring.
//!
//! This module is deliberately model-neutral. It turns the normalized CKB
//! dependency graph into bounded, provenance-preserving context slices that
//! can be consumed by MCP clients, IDE agents, GPT/Claude/Gemini-class models,
//! CI agents, or the CKB UI without dumping an entire repository into context.
//!
//! Retrieval is intentionally graph-aware and bounded. The neighborhood index
//! is built once per query in O(V + E), avoiding the older O(frontier * E)
//! repeated edge scans that became expensive on large repositories.

use crate::graph::DependencyGraph;
use crate::types::{Node, NodeId, RuntimeMetrics};
use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet, VecDeque};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MemoryEvidence {
    pub source: String,
    pub reference: String,
    pub kind: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MemoryNode {
    pub id: String,
    pub name: String,
    pub kind: String,
    pub path: String,
    pub line: u32,
    pub column: u32,
    pub score: f32,
    pub fan_in: usize,
    pub fan_out: usize,
    pub activity_priority: f32,
    pub runtime: Option<RuntimeMetrics>,
    pub evidence: Vec<MemoryEvidence>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MemoryEdge {
    pub source: String,
    pub target: String,
    pub kind: String,
    pub weight: f32,
    pub evidence: Vec<MemoryEvidence>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MemoryRetrievalStats {
    pub graph_nodes: usize,
    pub graph_edges: usize,
    pub root_count: usize,
    pub retrieved_nodes: usize,
    pub retrieved_edges: usize,
    pub runtime_observed_nodes: usize,
    pub expansion_cap: usize,
    pub truncated: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ArchitectureMemorySlice {
    pub version: String,
    pub query: String,
    pub depth: usize,
    pub nodes: Vec<MemoryNode>,
    pub edges: Vec<MemoryEdge>,
    pub root_ids: Vec<String>,
    pub context: String,
    pub retrieval: MemoryRetrievalStats,
    pub evidence_policy: String,
    pub synthetic: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CodeDnaNode {
    pub id: String,
    pub name: String,
    pub path: String,
    pub kind: String,
    pub fan_in: usize,
    pub fan_out: usize,
    pub instability: f32,
    pub structural_pressure: f32,
    pub runtime_pressure: f32,
    pub cycle_member: bool,
    pub health_score: f32,
    pub risk_score: f32,
    pub evidence: Vec<MemoryEvidence>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CodeDnaReport {
    pub version: String,
    pub overall_health: f32,
    pub nodes_analyzed: usize,
    pub cycle_count: usize,
    pub runtime_observed_nodes: usize,
    pub highest_risk: Vec<CodeDnaNode>,
    pub nodes: Vec<CodeDnaNode>,
    pub evidence_policy: String,
    pub synthetic: bool,
}

pub struct ArchitectureMemoryEngine;

impl ArchitectureMemoryEngine {
    fn kind<T: std::fmt::Debug>(value: T) -> String {
        format!("{:?}", value).to_ascii_lowercase()
    }

    fn terms(query: &str) -> Vec<String> {
        query
            .to_ascii_lowercase()
            .split(|c: char| !(c.is_ascii_alphanumeric() || matches!(c, '_' | '.' | '/' | ':' | '$' | '-')))
            .filter(|s| !s.is_empty())
            .map(str::to_string)
            .collect()
    }

    fn score_text(id: &str, name: &str, path: &str, query_terms: &[String]) -> f32 {
        if query_terms.is_empty() {
            return 1.0;
        }
        let id_l = id.to_ascii_lowercase();
        let name_l = name.to_ascii_lowercase();
        let path_l = path.to_ascii_lowercase();
        let all = format!("{} {} {}", id_l, name_l, path_l);
        let mut score = 0.0;
        for term in query_terms {
            if id_l == *term { score += 18.0; }
            if name_l == *term { score += 15.0; }
            if id_l.ends_with(&format!("::{}", term)) { score += 12.0; }
            if path_l == *term { score += 11.0; }
            if path_l.ends_with(term) { score += 8.0; }
            if path_l.contains(term) { score += 6.0; }
            if all.contains(term) { score += 2.0; }
        }
        score
    }

    fn normalized_log(value: f32, max: f32) -> f32 {
        if value <= 0.0 || max <= 0.0 {
            return 0.0;
        }
        ((1.0 + value).ln() / (1.0 + max).ln()).clamp(0.0, 1.0)
    }

    fn activity_priority(
        fan_in: usize,
        fan_out: usize,
        max_degree: usize,
        runtime: Option<&RuntimeMetrics>,
        max_calls: u64,
    ) -> f32 {
        let structural = Self::normalized_log((fan_in * 2 + fan_out) as f32, (max_degree.max(1) * 2) as f32);
        let runtime_intensity = runtime
            .map(|metrics| Self::normalized_log(metrics.execution_count as f32, max_calls.max(1) as f32))
            .unwrap_or(0.0);
        let runtime_risk = runtime
            .map(|metrics| {
                (metrics.error_rate.clamp(0.0, 1.0) * 0.65
                    + if metrics.is_hotpath { 0.20 } else { 0.0 }
                    + (metrics.avg_latency_ms / 5000.0).clamp(0.0, 0.15))
                    .clamp(0.0, 1.0)
            })
            .unwrap_or(0.0);
        (structural * 0.50 + runtime_intensity * 0.30 + runtime_risk * 0.20).clamp(0.0, 1.0)
    }

    fn memory_node(
        graph: &DependencyGraph,
        node: &Node,
        score: f32,
        fan_in: usize,
        fan_out: usize,
        max_degree: usize,
        max_calls: u64,
    ) -> MemoryNode {
        let runtime = graph.get_runtime_metrics(&node.id).cloned();
        let activity_priority = Self::activity_priority(
            fan_in,
            fan_out,
            max_degree,
            runtime.as_ref(),
            max_calls,
        );
        let mut evidence = vec![MemoryEvidence {
            source: "tree-sitter-ast".into(),
            reference: format!("{}:{}:{}", node.path.to_string_lossy(), node.line, node.column),
            kind: "static".into(),
        }, MemoryEvidence {
            source: "dependency-graph".into(),
            reference: format!("fan-in={fan_in};fan-out={fan_out}"),
            kind: "static".into(),
        }];
        if runtime.is_some() {
            evidence.push(MemoryEvidence {
                source: "runtime-telemetry".into(),
                reference: node.id.0.clone(),
                kind: "runtime".into(),
            });
        }
        MemoryNode {
            id: node.id.0.clone(),
            name: node.name.clone(),
            kind: Self::kind(node.kind),
            path: node.path.to_string_lossy().to_string(),
            line: node.line,
            column: node.column,
            score,
            fan_in,
            fan_out,
            activity_priority,
            runtime,
            evidence,
        }
    }

    fn memory_edge(edge: &crate::types::Edge) -> MemoryEdge {
        let mut evidence = vec![MemoryEvidence {
            source: "ckb-graph".into(),
            reference: format!("{}->{}", edge.from.0, edge.to.0),
            kind: "static".into(),
        }];
        if let Some(resolution) = edge.metadata.get("resolution") {
            evidence.push(MemoryEvidence {
                source: "semantic-resolution".into(),
                reference: resolution.clone(),
                kind: "static".into(),
            });
        }
        MemoryEdge {
            source: edge.from.0.clone(),
            target: edge.to.0.clone(),
            kind: Self::kind(edge.kind),
            weight: edge.weight,
            evidence,
        }
    }

    /// Retrieve a bounded architecture-memory neighborhood around the symbols
    /// that best match a natural-language/symbol/path query.
    ///
    /// Performance note: nodes, degree maps, and adjacency are indexed once.
    /// Expansion therefore scales with the retrieved neighborhood instead of
    /// rescanning every edge for every frontier node.
    pub fn query(graph: &DependencyGraph, query: &str, depth: usize, limit: usize) -> Result<ArchitectureMemorySlice> {
        let depth = depth.min(8);
        let limit = limit.clamp(1, 250);
        let query_terms = Self::terms(query);
        let graph_nodes = graph.nodes();
        let graph_edges = graph.edges();

        let mut node_by_id: HashMap<NodeId, &Node> = HashMap::with_capacity(graph_nodes.len());
        let mut incoming: HashMap<NodeId, usize> = HashMap::with_capacity(graph_nodes.len());
        let mut outgoing: HashMap<NodeId, usize> = HashMap::with_capacity(graph_nodes.len());
        let mut adjacency: HashMap<NodeId, Vec<NodeId>> = HashMap::with_capacity(graph_nodes.len());
        for node in &graph_nodes {
            node_by_id.insert(node.id.clone(), node);
            incoming.insert(node.id.clone(), 0);
            outgoing.insert(node.id.clone(), 0);
            adjacency.insert(node.id.clone(), Vec::new());
        }
        for edge in &graph_edges {
            *outgoing.entry(edge.from.clone()).or_insert(0) += 1;
            *incoming.entry(edge.to.clone()).or_insert(0) += 1;
            adjacency.entry(edge.from.clone()).or_default().push(edge.to.clone());
            adjacency.entry(edge.to.clone()).or_default().push(edge.from.clone());
        }

        let max_degree = graph_nodes.iter().map(|node| {
            incoming.get(&node.id).copied().unwrap_or(0) + outgoing.get(&node.id).copied().unwrap_or(0)
        }).max().unwrap_or(0);
        let max_calls = graph_nodes.iter()
            .filter_map(|node| graph.get_runtime_metrics(&node.id))
            .map(|runtime| runtime.execution_count)
            .max()
            .unwrap_or(0);

        let mut ranked: Vec<(NodeId, f32)> = graph_nodes.iter()
            .map(|node| {
                let path = node.path.to_string_lossy();
                let lexical = Self::score_text(&node.id.0, &node.name, &path, &query_terms);
                let fan_in = incoming.get(&node.id).copied().unwrap_or(0);
                let fan_out = outgoing.get(&node.id).copied().unwrap_or(0);
                let activity = Self::activity_priority(
                    fan_in,
                    fan_out,
                    max_degree,
                    graph.get_runtime_metrics(&node.id),
                    max_calls,
                );
                let score = if query_terms.is_empty() {
                    1.0 + activity * 4.0
                } else if lexical > 0.0 {
                    lexical + activity * 2.0
                } else {
                    0.0
                };
                (node.id.clone(), score)
            })
            .filter(|(_, score)| *score > 0.0)
            .collect();
        ranked.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal).then_with(|| a.0.0.cmp(&b.0.0)));

        // A memory query should have a focused root set even if the caller asks
        // for a large result limit; the limit mainly controls returned context.
        let root_budget = if query_terms.is_empty() { limit.min(48) } else { limit.min(24) };
        ranked.truncate(root_budget.max(1));

        let roots: Vec<NodeId> = ranked.iter().map(|(id, _)| id.clone()).collect();
        let root_scores: HashMap<NodeId, f32> = ranked.into_iter().collect();
        let expansion_cap = (limit.max(12) * 40).clamp(200, 2500).min(graph_nodes.len().max(1));
        let mut included: HashSet<NodeId> = roots.iter().cloned().collect();
        let mut frontier: VecDeque<(NodeId, usize)> = roots.iter().cloned().map(|id| (id, 0)).collect();
        let mut truncated = false;

        while let Some((current, current_depth)) = frontier.pop_front() {
            if current_depth >= depth { continue; }
            let mut neighbors = adjacency.get(&current).cloned().unwrap_or_default();
            neighbors.sort_by(|a, b| {
                let a_degree = incoming.get(a).copied().unwrap_or(0) + outgoing.get(a).copied().unwrap_or(0);
                let b_degree = incoming.get(b).copied().unwrap_or(0) + outgoing.get(b).copied().unwrap_or(0);
                b_degree.cmp(&a_degree).then_with(|| a.0.cmp(&b.0))
            });
            for next in neighbors {
                if included.len() >= expansion_cap {
                    truncated = true;
                    break;
                }
                if included.insert(next.clone()) {
                    frontier.push_back((next, current_depth + 1));
                }
            }
            if truncated { break; }
        }

        let mut nodes = included.iter().filter_map(|id| {
            let node = node_by_id.get(id).copied()?;
            let fan_in = incoming.get(id).copied().unwrap_or(0);
            let fan_out = outgoing.get(id).copied().unwrap_or(0);
            let root_score = root_scores.get(id).copied().unwrap_or(0.0);
            let activity = Self::activity_priority(fan_in, fan_out, max_degree, graph.get_runtime_metrics(id), max_calls);
            // Neighbor nodes retain a small evidence-priority score so model
            // context orders architecture hubs ahead of incidental leaves.
            let score = if root_score > 0.0 { root_score } else { activity };
            Some(Self::memory_node(graph, node, score, fan_in, fan_out, max_degree, max_calls))
        }).collect::<Vec<_>>();
        nodes.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| b.activity_priority.partial_cmp(&a.activity_priority).unwrap_or(std::cmp::Ordering::Equal))
            .then_with(|| a.id.cmp(&b.id)));

        let edges = graph_edges.iter()
            .filter(|edge| included.contains(&edge.from) && included.contains(&edge.to))
            .map(|edge| Self::memory_edge(edge))
            .collect::<Vec<_>>();

        let runtime_observed_nodes = nodes.iter().filter(|node| node.runtime.is_some()).count();
        let root_ids = roots.iter().map(|id| id.0.clone()).collect::<Vec<_>>();
        let retrieval = MemoryRetrievalStats {
            graph_nodes: graph_nodes.len(),
            graph_edges: graph_edges.len(),
            root_count: root_ids.len(),
            retrieved_nodes: nodes.len(),
            retrieved_edges: edges.len(),
            runtime_observed_nodes,
            expansion_cap,
            truncated,
        };
        let context = Self::render_context(query, &nodes, &edges, &root_ids, &retrieval);

        Ok(ArchitectureMemorySlice {
            version: "ckb-architecture-memory-v3".into(),
            query: query.into(),
            depth,
            nodes,
            edges,
            root_ids,
            context,
            retrieval,
            evidence_policy: "static-runtime-predicted-separated".into(),
            synthetic: false,
        })
    }

    fn render_context(
        query: &str,
        nodes: &[MemoryNode],
        edges: &[MemoryEdge],
        roots: &[String],
        retrieval: &MemoryRetrievalStats,
    ) -> String {
        let mut out = String::new();
        out.push_str("# CKB Retrieved Architecture Memory\n");
        out.push_str(&format!("Query: {}\n", query));
        out.push_str("Evidence policy: static relationships are not proof of runtime execution.\n");
        out.push_str(&format!("Root symbols: {}\n", roots.join(", ")));
        out.push_str(&format!(
            "Graph coverage: {} of {} nodes, {} of {} relationships, {} runtime-observed nodes{}\n\n",
            retrieval.retrieved_nodes,
            retrieval.graph_nodes,
            retrieval.retrieved_edges,
            retrieval.graph_edges,
            retrieval.runtime_observed_nodes,
            if retrieval.truncated { " (bounded retrieval cap reached)" } else { "" },
        ));
        for node in nodes.iter().take(120) {
            out.push_str(&format!("## {} [{}]\n", node.name, node.kind));
            out.push_str(&format!(
                "id: {}\nsource: {}:{}:{}\nfan-in: {}\nfan-out: {}\nactivity-priority: {:.3}\n",
                node.id,
                node.path,
                node.line,
                node.column,
                node.fan_in,
                node.fan_out,
                node.activity_priority,
            ));
            if let Some(runtime) = &node.runtime {
                out.push_str(&format!(
                    "runtime-observed: calls={}, avg_latency_ms={:.2}, error_rate={:.4}, hotpath={}\n",
                    runtime.execution_count,
                    runtime.avg_latency_ms,
                    runtime.error_rate,
                    runtime.is_hotpath,
                ));
            } else {
                out.push_str("runtime-observed: no telemetry attached to this symbol\n");
            }
        }
        out.push_str("\n## Relationships\n");
        for edge in edges.iter().take(240) {
            out.push_str(&format!("{} --{}--> {}\n", edge.source, edge.kind, edge.target));
        }
        out
    }

    /// Produce deterministic, explainable Code DNA scores from the current
    /// graph and observed runtime telemetry. These are heuristic engineering
    /// health indices, not learned failure probabilities.
    pub fn code_dna(graph: &DependencyGraph) -> Result<CodeDnaReport> {
        let nodes = graph.nodes();
        let edges = graph.edges();
        let cycles = graph.find_cycles()?;
        let cycle_members: HashSet<NodeId> = cycles.iter().flat_map(|cycle| cycle.iter().cloned()).collect();
        let total_nodes = nodes.len().max(1) as f32;
        let mut incoming: HashMap<NodeId, usize> = HashMap::with_capacity(nodes.len());
        let mut outgoing: HashMap<NodeId, usize> = HashMap::with_capacity(nodes.len());
        for node in &nodes {
            incoming.insert(node.id.clone(), 0);
            outgoing.insert(node.id.clone(), 0);
        }
        for edge in &edges {
            *outgoing.entry(edge.from.clone()).or_insert(0) += 1;
            *incoming.entry(edge.to.clone()).or_insert(0) += 1;
        }

        let mut runtime_observed_nodes = 0usize;
        let mut result = Vec::with_capacity(nodes.len());

        for node in nodes {
            let fan_in = incoming.get(&node.id).copied().unwrap_or(0);
            let fan_out = outgoing.get(&node.id).copied().unwrap_or(0);
            let degree = fan_in + fan_out;
            let instability = if degree == 0 { 0.0 } else { fan_out as f32 / degree as f32 };
            let centrality = ((fan_in as f32 * 2.0 + fan_out as f32) / (total_nodes * 0.35).max(1.0)).min(1.0);
            let cycle_pressure = if cycle_members.contains(&node.id) { 0.28 } else { 0.0 };
            let structural_pressure = (centrality * 0.55 + instability * 0.17 + cycle_pressure).min(1.0);

            let runtime = graph.get_runtime_metrics(&node.id);
            let runtime_pressure = runtime.map(|metrics| {
                runtime_observed_nodes += 1;
                let call_pressure = ((metrics.execution_count as f32 + 1.0).log10() / 6.0).min(0.25);
                let latency_pressure = (metrics.avg_latency_ms / 2500.0).min(0.25);
                let error_pressure = (metrics.error_rate * 3.0).min(0.35);
                let hotpath_pressure = if metrics.is_hotpath { 0.15 } else { 0.0 };
                (call_pressure + latency_pressure + error_pressure + hotpath_pressure).min(1.0)
            }).unwrap_or(0.0);

            let risk = (structural_pressure * 0.68 + runtime_pressure * 0.32).min(1.0);
            let health = ((1.0 - risk) * 100.0).clamp(0.0, 100.0);
            let mut evidence = vec![MemoryEvidence {
                source: "dependency-graph".into(),
                reference: format!("fan-in={} fan-out={}", fan_in, fan_out),
                kind: "static".into(),
            }];
            if cycle_members.contains(&node.id) {
                evidence.push(MemoryEvidence {
                    source: "scc-cycle-analysis".into(),
                    reference: node.id.0.clone(),
                    kind: "static".into(),
                });
            }
            if runtime.is_some() {
                evidence.push(MemoryEvidence {
                    source: "runtime-telemetry".into(),
                    reference: node.id.0.clone(),
                    kind: "runtime".into(),
                });
            }

            result.push(CodeDnaNode {
                id: node.id.0.clone(),
                name: node.name.clone(),
                path: node.path.to_string_lossy().to_string(),
                kind: Self::kind(node.kind),
                fan_in,
                fan_out,
                instability,
                structural_pressure,
                runtime_pressure,
                cycle_member: cycle_members.contains(&node.id),
                health_score: health,
                risk_score: risk,
                evidence,
            });
        }

        result.sort_by(|a, b| b.risk_score.partial_cmp(&a.risk_score).unwrap_or(std::cmp::Ordering::Equal));
        let overall_health = if result.is_empty() {
            100.0
        } else {
            result.iter().map(|node| node.health_score).sum::<f32>() / result.len() as f32
        };
        let highest_risk = result.iter().take(20).cloned().collect();

        Ok(CodeDnaReport {
            version: "ckb-code-dna-v3".into(),
            overall_health,
            nodes_analyzed: result.len(),
            cycle_count: cycles.len(),
            runtime_observed_nodes,
            highest_risk,
            nodes: result,
            evidence_policy: "static-runtime-separated; scores-are-explainable-heuristics".into(),
            synthetic: false,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::ArchitectureMemoryEngine;

    #[test]
    fn tokenization_preserves_symbol_path_tokens() {
        let terms = ArchitectureMemoryEngine::terms("PaymentService.charge src/payments/api.ts");
        assert!(terms.contains(&"paymentservice.charge".to_string()));
        assert!(terms.contains(&"src/payments/api.ts".to_string()));
    }

    #[test]
    fn exact_symbol_match_scores_above_path_contains() {
        let terms = vec!["charge".to_string()];
        let exact = ArchitectureMemoryEngine::score_text("src/a.ts::charge", "charge", "src/a.ts", &terms);
        let partial = ArchitectureMemoryEngine::score_text("src/charge/a.ts::file", "a.ts", "src/charge/a.ts", &terms);
        assert!(exact > partial);
    }

    #[test]
    fn normalized_log_is_bounded() {
        assert_eq!(ArchitectureMemoryEngine::normalized_log(0.0, 100.0), 0.0);
        assert_eq!(ArchitectureMemoryEngine::normalized_log(100.0, 100.0), 1.0);
        assert!((0.0..=1.0).contains(&ArchitectureMemoryEngine::normalized_log(12.0, 100.0)));
    }
}
