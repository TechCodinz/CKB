//! Evidence-backed architecture memory retrieval and Code DNA scoring.
//!
//! This module is deliberately model-neutral. It turns the normalized CKB
//! dependency graph into bounded, provenance-preserving context slices that
//! can be consumed by MCP clients, IDE agents, GPT/Claude/Gemini-class models,
//! CI agents, or the CKB UI without dumping an entire repository into context.

use crate::graph::DependencyGraph;
use crate::types::{EdgeKind, NodeId, RuntimeMetrics};
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
pub struct ArchitectureMemorySlice {
    pub version: String,
    pub query: String,
    pub depth: usize,
    pub nodes: Vec<MemoryNode>,
    pub edges: Vec<MemoryEdge>,
    pub root_ids: Vec<String>,
    pub context: String,
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
            if id_l == *term { score += 16.0; }
            if name_l == *term { score += 13.0; }
            if id_l.ends_with(&format!("::{}", term)) { score += 10.0; }
            if path_l == *term { score += 9.0; }
            if path_l.contains(term) { score += 6.0; }
            if all.contains(term) { score += 2.0; }
        }
        score
    }

    fn memory_node(graph: &DependencyGraph, id: &NodeId, score: f32) -> Option<MemoryNode> {
        let node = graph.nodes().into_iter().find(|n| n.id == *id)?;
        let runtime = graph.get_runtime_metrics(id).cloned();
        let mut evidence = vec![MemoryEvidence {
            source: "tree-sitter-ast".into(),
            reference: format!("{}:{}:{}", node.path.to_string_lossy(), node.line, node.column),
            kind: "static".into(),
        }];
        if runtime.is_some() {
            evidence.push(MemoryEvidence {
                source: "runtime-telemetry".into(),
                reference: node.id.0.clone(),
                kind: "runtime".into(),
            });
        }
        Some(MemoryNode {
            id: node.id.0.clone(),
            name: node.name.clone(),
            kind: Self::kind(node.kind),
            path: node.path.to_string_lossy().to_string(),
            line: node.line,
            column: node.column,
            score,
            runtime,
            evidence,
        })
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
    pub fn query(graph: &DependencyGraph, query: &str, depth: usize, limit: usize) -> Result<ArchitectureMemorySlice> {
        let depth = depth.min(5);
        let limit = limit.clamp(1, 100);
        let query_terms = Self::terms(query);

        let mut ranked: Vec<(NodeId, f32)> = graph.nodes().into_iter()
            .map(|n| {
                let path = n.path.to_string_lossy();
                let mut score = Self::score_text(&n.id.0, &n.name, &path, &query_terms);
                if graph.get_runtime_metrics(&n.id).is_some() { score += 0.5; }
                (n.id.clone(), score)
            })
            .filter(|(_, score)| *score > 0.0 || query_terms.is_empty())
            .collect();
        ranked.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        ranked.truncate(limit);

        let roots: Vec<NodeId> = ranked.iter().map(|(id, _)| id.clone()).collect();
        let root_scores: HashMap<NodeId, f32> = ranked.into_iter().collect();
        let mut included: HashSet<NodeId> = roots.iter().cloned().collect();
        let mut frontier: VecDeque<(NodeId, usize)> = roots.iter().cloned().map(|id| (id, 0)).collect();

        while let Some((current, d)) = frontier.pop_front() {
            if d >= depth { continue; }
            for edge in graph.edges() {
                let neighbor = if edge.from == current {
                    Some(edge.to.clone())
                } else if edge.to == current {
                    Some(edge.from.clone())
                } else {
                    None
                };
                if let Some(next) = neighbor {
                    if included.len() >= 500 { break; }
                    if included.insert(next.clone()) {
                        frontier.push_back((next, d + 1));
                    }
                }
            }
        }

        let mut nodes = Vec::new();
        for id in &included {
            if let Some(n) = Self::memory_node(graph, id, *root_scores.get(id).unwrap_or(&0.0)) {
                nodes.push(n);
            }
        }
        nodes.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal).then_with(|| a.id.cmp(&b.id)));

        let edges = graph.edges().into_iter()
            .filter(|e| included.contains(&e.from) && included.contains(&e.to))
            .map(Self::memory_edge)
            .collect::<Vec<_>>();

        let root_ids = roots.iter().map(|id| id.0.clone()).collect::<Vec<_>>();
        let context = Self::render_context(query, &nodes, &edges, &root_ids);

        Ok(ArchitectureMemorySlice {
            version: "ckb-architecture-memory-v2".into(),
            query: query.into(),
            depth,
            nodes,
            edges,
            root_ids,
            context,
            evidence_policy: "static-runtime-predicted-separated".into(),
            synthetic: false,
        })
    }

    fn render_context(query: &str, nodes: &[MemoryNode], edges: &[MemoryEdge], roots: &[String]) -> String {
        let mut out = String::new();
        out.push_str("# CKB Retrieved Architecture Memory\n");
        out.push_str(&format!("Query: {}\n", query));
        out.push_str("Evidence policy: static relationships are not proof of runtime execution.\n");
        out.push_str(&format!("Root symbols: {}\n", roots.join(", ")));
        out.push_str(&format!("Retrieved: {} nodes, {} relationships\n\n", nodes.len(), edges.len()));
        for n in nodes.iter().take(80) {
            out.push_str(&format!("## {} [{}]\n", n.name, n.kind));
            out.push_str(&format!("id: {}\nsource: {}:{}:{}\n", n.id, n.path, n.line, n.column));
            if let Some(r) = &n.runtime {
                out.push_str(&format!("runtime-observed: calls={}, avg_latency_ms={:.2}, error_rate={:.4}, hotpath={}\n", r.execution_count, r.avg_latency_ms, r.error_rate, r.is_hotpath));
            }
        }
        out.push_str("\n## Relationships\n");
        for e in edges.iter().take(160) {
            out.push_str(&format!("{} --{}--> {}\n", e.source, e.kind, e.target));
        }
        out
    }

    /// Produce deterministic, explainable Code DNA scores from the current
    /// graph and observed runtime telemetry. These are heuristic engineering
    /// health indices, not learned failure probabilities.
    pub fn code_dna(graph: &DependencyGraph) -> Result<CodeDnaReport> {
        let cycles = graph.find_cycles()?;
        let cycle_members: HashSet<NodeId> = cycles.iter().flat_map(|c| c.iter().cloned()).collect();
        let total_nodes = graph.node_count().max(1) as f32;
        let mut runtime_observed_nodes = 0usize;
        let mut result = Vec::new();

        for n in graph.nodes() {
            let fan_in = graph.incoming_degree(&n.id)?;
            let fan_out = graph.outgoing_degree(&n.id)?;
            let degree = fan_in + fan_out;
            let instability = if degree == 0 { 0.0 } else { fan_out as f32 / degree as f32 };
            let centrality = ((fan_in as f32 * 2.0 + fan_out as f32) / (total_nodes * 0.35).max(1.0)).min(1.0);
            let cycle_pressure = if cycle_members.contains(&n.id) { 0.28 } else { 0.0 };
            let structural_pressure = (centrality * 0.55 + instability * 0.17 + cycle_pressure).min(1.0);

            let runtime = graph.get_runtime_metrics(&n.id);
            let runtime_pressure = runtime.map(|r| {
                runtime_observed_nodes += 1;
                let call_pressure = ((r.execution_count as f32 + 1.0).log10() / 6.0).min(0.25);
                let latency_pressure = (r.avg_latency_ms / 2500.0).min(0.25);
                let error_pressure = (r.error_rate * 3.0).min(0.35);
                let hotpath_pressure = if r.is_hotpath { 0.15 } else { 0.0 };
                (call_pressure + latency_pressure + error_pressure + hotpath_pressure).min(1.0)
            }).unwrap_or(0.0);

            let risk = (structural_pressure * 0.68 + runtime_pressure * 0.32).min(1.0);
            let health = ((1.0 - risk) * 100.0).clamp(0.0, 100.0);
            let mut evidence = vec![MemoryEvidence {
                source: "dependency-graph".into(),
                reference: format!("fan-in={} fan-out={}", fan_in, fan_out),
                kind: "static".into(),
            }];
            if cycle_members.contains(&n.id) {
                evidence.push(MemoryEvidence { source: "scc-cycle-analysis".into(), reference: n.id.0.clone(), kind: "static".into() });
            }
            if runtime.is_some() {
                evidence.push(MemoryEvidence { source: "runtime-telemetry".into(), reference: n.id.0.clone(), kind: "runtime".into() });
            }

            result.push(CodeDnaNode {
                id: n.id.0.clone(),
                name: n.name.clone(),
                path: n.path.to_string_lossy().to_string(),
                kind: Self::kind(n.kind),
                fan_in,
                fan_out,
                instability,
                structural_pressure,
                runtime_pressure,
                cycle_member: cycle_members.contains(&n.id),
                health_score: health,
                risk_score: risk,
                evidence,
            });
        }

        result.sort_by(|a, b| b.risk_score.partial_cmp(&a.risk_score).unwrap_or(std::cmp::Ordering::Equal));
        let overall_health = if result.is_empty() {
            100.0
        } else {
            result.iter().map(|n| n.health_score).sum::<f32>() / result.len() as f32
        };
        let highest_risk = result.iter().take(20).cloned().collect();

        Ok(CodeDnaReport {
            version: "ckb-code-dna-v2".into(),
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
}
