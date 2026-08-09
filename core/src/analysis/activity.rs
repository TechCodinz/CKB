//! Deep activity-oriented architecture analysis.
//!
//! This module fuses static graph structure with observed runtime node telemetry
//! into explainable engineering indices. It is intentionally model-neutral and
//! never treats a structural relationship as proof that a runtime call occurred.

use crate::graph::DependencyGraph;
use crate::types::{NodeId, RuntimeMetrics};
use anyhow::Result;
use chrono::Utc;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ActivityEvidence {
    pub source: String,
    pub reference: String,
    pub kind: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ActivityNode {
    pub id: String,
    pub name: String,
    pub kind: String,
    pub path: String,
    pub line: u32,
    pub fan_in: usize,
    pub fan_out: usize,
    pub instability: f32,
    pub structural_centrality: f32,
    pub runtime_observed: bool,
    pub invocation_count: u64,
    pub avg_latency_ms: f32,
    pub error_rate: f32,
    pub is_hotpath: bool,
    pub activity_index: f32,
    pub change_sensitivity_index: f32,
    pub operational_pressure_index: f32,
    pub role: String,
    pub evidence: Vec<ActivityEvidence>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ActivityBoundary {
    pub id: String,
    pub kind: String,
    pub symbols: usize,
    pub incoming_cross_boundary: usize,
    pub outgoing_cross_boundary: usize,
    pub runtime_observed_symbols: usize,
    pub activity_index: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeepActivityReport {
    pub version: String,
    pub generated_at: String,
    pub nodes_analyzed: usize,
    pub edges_analyzed: usize,
    pub cycle_count: usize,
    pub boundary_count: usize,
    pub runtime_observed_nodes: usize,
    pub runtime_coverage_pct: f32,
    pub hotspots: Vec<ActivityNode>,
    pub change_sensitive: Vec<ActivityNode>,
    pub operational_hotspots: Vec<ActivityNode>,
    pub boundaries: Vec<ActivityBoundary>,
    pub memory_priority_ids: Vec<String>,
    pub scoring_policy: String,
    pub evidence_policy: String,
    pub synthetic: bool,
}

pub struct DeepActivityAnalyzer;

impl DeepActivityAnalyzer {
    fn kind<T: std::fmt::Debug>(value: T) -> String {
        format!("{:?}", value).to_ascii_lowercase()
    }

    fn boundary(path: &str) -> String {
        let normalized = path.replace('\\', "/");
        let parts = normalized.split('/').filter(|part| !part.is_empty()).collect::<Vec<_>>();
        if parts.is_empty() {
            return "workspace-root".into();
        }
        let first = parts[0].to_ascii_lowercase();
        if matches!(first.as_str(), "apps" | "services" | "packages" | "libs" | "crates" | "modules") && parts.len() > 1 {
            return format!("{}/{}", parts[0], parts[1]);
        }
        parts[0].to_string()
    }

    fn boundary_kind(boundary: &str) -> String {
        let prefix = boundary.split('/').next().unwrap_or("").to_ascii_lowercase();
        if matches!(prefix.as_str(), "apps" | "services") {
            "service-boundary".into()
        } else if matches!(prefix.as_str(), "packages" | "libs" | "crates" | "modules") {
            "package-boundary".into()
        } else {
            "directory-boundary".into()
        }
    }

    fn runtime_tuple(runtime: Option<&RuntimeMetrics>) -> (bool, u64, f32, f32, bool) {
        match runtime {
            Some(value) => (
                true,
                value.execution_count,
                value.avg_latency_ms,
                value.error_rate,
                value.is_hotpath,
            ),
            None => (false, 0, 0.0, 0.0, false),
        }
    }

    fn normalized_log(value: f32, max: f32) -> f32 {
        if value <= 0.0 || max <= 0.0 {
            return 0.0;
        }
        ((1.0 + value).ln() / (1.0 + max).ln()).clamp(0.0, 1.0)
    }

    fn role(fan_in: usize, fan_out: usize, cycle_member: bool, runtime: Option<&RuntimeMetrics>) -> String {
        if cycle_member {
            return "cycle-core".into();
        }
        if fan_in >= 8 && fan_out >= 8 {
            return "architecture-hub".into();
        }
        if fan_in >= 8 {
            return "shared-contract".into();
        }
        if fan_out >= 8 {
            return "orchestrator".into();
        }
        if runtime.map(|value| value.is_hotpath).unwrap_or(false) {
            return "runtime-hotpath".into();
        }
        if fan_in == 0 && fan_out > 0 {
            return "entry-or-leaf-source".into();
        }
        if fan_in > 0 && fan_out == 0 {
            return "terminal-dependency".into();
        }
        "connected-symbol".into()
    }

    /// Analyze the current graph in O(V + E) for degree/boundary aggregation,
    /// plus the graph's cycle detector. Runtime observations are folded into
    /// activity and operational-pressure indices without inventing telemetry.
    pub fn analyze(graph: &DependencyGraph) -> Result<DeepActivityReport> {
        let nodes = graph.nodes();
        let edges = graph.edges();
        let node_count = nodes.len();

        let mut incoming: HashMap<NodeId, usize> = HashMap::with_capacity(node_count);
        let mut outgoing: HashMap<NodeId, usize> = HashMap::with_capacity(node_count);
        for node in &nodes {
            incoming.insert(node.id.clone(), 0);
            outgoing.insert(node.id.clone(), 0);
        }
        for edge in &edges {
            *outgoing.entry(edge.from.clone()).or_insert(0) += 1;
            *incoming.entry(edge.to.clone()).or_insert(0) += 1;
        }

        let cycles = graph.find_cycles()?;
        let cycle_members: HashSet<NodeId> = cycles.iter().flat_map(|cycle| cycle.iter().cloned()).collect();

        let max_degree = nodes.iter().map(|node| {
            incoming.get(&node.id).copied().unwrap_or(0) + outgoing.get(&node.id).copied().unwrap_or(0)
        }).max().unwrap_or(0) as f32;
        let max_calls = nodes.iter().filter_map(|node| graph.get_runtime_metrics(&node.id)).map(|runtime| runtime.execution_count).max().unwrap_or(0) as f32;
        let max_latency = nodes.iter().filter_map(|node| graph.get_runtime_metrics(&node.id)).map(|runtime| runtime.avg_latency_ms).fold(0.0_f32, f32::max);

        let mut analyzed = Vec::with_capacity(node_count);
        let mut runtime_observed_nodes = 0usize;

        for node in &nodes {
            let fan_in = incoming.get(&node.id).copied().unwrap_or(0);
            let fan_out = outgoing.get(&node.id).copied().unwrap_or(0);
            let degree = fan_in + fan_out;
            let instability = if degree == 0 { 0.0 } else { fan_out as f32 / degree as f32 };
            let structural_centrality = Self::normalized_log((fan_in * 2 + fan_out) as f32, (max_degree * 2.0).max(1.0));
            let runtime = graph.get_runtime_metrics(&node.id);
            let (runtime_observed, invocation_count, avg_latency_ms, error_rate, is_hotpath) = Self::runtime_tuple(runtime);
            if runtime_observed {
                runtime_observed_nodes += 1;
            }

            let call_intensity = Self::normalized_log(invocation_count as f32, max_calls);
            let latency_intensity = if max_latency > 0.0 { (avg_latency_ms / max_latency).clamp(0.0, 1.0) } else { 0.0 };
            let error_intensity = error_rate.clamp(0.0, 1.0);
            let cycle_pressure = if cycle_members.contains(&node.id) { 1.0 } else { 0.0 };
            let fan_in_pressure = Self::normalized_log(fan_in as f32, max_degree.max(1.0));
            let fan_out_pressure = Self::normalized_log(fan_out as f32, max_degree.max(1.0));

            let activity_index = (
                structural_centrality * 0.34
                    + call_intensity * 0.36
                    + latency_intensity * 0.10
                    + error_intensity * 0.15
                    + if is_hotpath { 0.05 } else { 0.0 }
            ).clamp(0.0, 1.0);

            let change_sensitivity_index = (
                fan_in_pressure * 0.48
                    + structural_centrality * 0.24
                    + cycle_pressure * 0.18
                    + call_intensity * 0.10
            ).clamp(0.0, 1.0);

            let operational_pressure_index = if runtime_observed {
                (
                    call_intensity * 0.35
                        + latency_intensity * 0.25
                        + error_intensity * 0.30
                        + if is_hotpath { 0.10 } else { 0.0 }
                ).clamp(0.0, 1.0)
            } else {
                0.0
            };

            let mut evidence = vec![ActivityEvidence {
                source: "ckb-dependency-graph".into(),
                reference: format!("fan-in={fan_in};fan-out={fan_out}"),
                kind: "static".into(),
            }];
            if cycle_members.contains(&node.id) {
                evidence.push(ActivityEvidence {
                    source: "scc-cycle-analysis".into(),
                    reference: node.id.0.clone(),
                    kind: "static".into(),
                });
            }
            if runtime_observed {
                evidence.push(ActivityEvidence {
                    source: "runtime-telemetry".into(),
                    reference: format!(
                        "calls={invocation_count};avg-latency-ms={avg_latency_ms:.2};error-rate={error_rate:.5};hotpath={is_hotpath}"
                    ),
                    kind: "runtime".into(),
                });
            }

            analyzed.push(ActivityNode {
                id: node.id.0.clone(),
                name: node.name.clone(),
                kind: Self::kind(node.kind),
                path: node.path.to_string_lossy().to_string(),
                line: node.line,
                fan_in,
                fan_out,
                instability,
                structural_centrality,
                runtime_observed,
                invocation_count,
                avg_latency_ms,
                error_rate,
                is_hotpath,
                activity_index,
                change_sensitivity_index,
                operational_pressure_index,
                role: Self::role(fan_in, fan_out, cycle_members.contains(&node.id), runtime),
                evidence,
            });
        }

        let mut hotspots = analyzed.clone();
        hotspots.sort_by(|a, b| b.activity_index.partial_cmp(&a.activity_index).unwrap_or(std::cmp::Ordering::Equal));
        hotspots.truncate(40);

        let mut change_sensitive = analyzed.clone();
        change_sensitive.sort_by(|a, b| b.change_sensitivity_index.partial_cmp(&a.change_sensitivity_index).unwrap_or(std::cmp::Ordering::Equal));
        change_sensitive.truncate(40);

        let mut operational_hotspots = analyzed.iter().filter(|node| node.runtime_observed).cloned().collect::<Vec<_>>();
        operational_hotspots.sort_by(|a, b| b.operational_pressure_index.partial_cmp(&a.operational_pressure_index).unwrap_or(std::cmp::Ordering::Equal));
        operational_hotspots.truncate(40);

        let mut node_boundaries: HashMap<NodeId, String> = HashMap::with_capacity(node_count);
        let mut boundary_counts: HashMap<String, (usize, usize, f32)> = HashMap::new();
        for node in &analyzed {
            let boundary = Self::boundary(&node.path);
            node_boundaries.insert(NodeId(node.id.clone()), boundary.clone());
            let entry = boundary_counts.entry(boundary).or_insert((0, 0, 0.0));
            entry.0 += 1;
            if node.runtime_observed { entry.1 += 1; }
            entry.2 += node.activity_index;
        }

        let mut boundary_cross: HashMap<String, (usize, usize)> = HashMap::new();
        for edge in &edges {
            let Some(source_boundary) = node_boundaries.get(&edge.from) else { continue; };
            let Some(target_boundary) = node_boundaries.get(&edge.to) else { continue; };
            if source_boundary == target_boundary { continue; }
            boundary_cross.entry(source_boundary.clone()).or_insert((0, 0)).1 += 1;
            boundary_cross.entry(target_boundary.clone()).or_insert((0, 0)).0 += 1;
        }

        let mut boundaries = boundary_counts.into_iter().map(|(id, (symbols, runtime_symbols, activity_sum))| {
            let (incoming_cross_boundary, outgoing_cross_boundary) = boundary_cross.get(&id).copied().unwrap_or((0, 0));
            ActivityBoundary {
                kind: Self::boundary_kind(&id),
                id,
                symbols,
                incoming_cross_boundary,
                outgoing_cross_boundary,
                runtime_observed_symbols: runtime_symbols,
                activity_index: if symbols == 0 { 0.0 } else { (activity_sum / symbols as f32).clamp(0.0, 1.0) },
            }
        }).collect::<Vec<_>>();
        boundaries.sort_by(|a, b| b.activity_index.partial_cmp(&a.activity_index).unwrap_or(std::cmp::Ordering::Equal).then_with(|| b.symbols.cmp(&a.symbols)));

        let mut priority = Vec::new();
        let mut seen = HashSet::new();
        for node in hotspots.iter().chain(change_sensitive.iter()).chain(operational_hotspots.iter()) {
            if seen.insert(node.id.clone()) {
                priority.push(node.id.clone());
            }
            if priority.len() >= 96 { break; }
        }

        let runtime_coverage_pct = if node_count == 0 {
            0.0
        } else {
            (runtime_observed_nodes as f32 / node_count as f32 * 100.0).clamp(0.0, 100.0)
        };

        Ok(DeepActivityReport {
            version: "ckb-deep-activity-v1".into(),
            generated_at: Utc::now().to_rfc3339(),
            nodes_analyzed: node_count,
            edges_analyzed: edges.len(),
            cycle_count: cycles.len(),
            boundary_count: boundaries.len(),
            runtime_observed_nodes,
            runtime_coverage_pct,
            hotspots,
            change_sensitive,
            operational_hotspots,
            boundaries,
            memory_priority_ids: priority,
            scoring_policy: "explainable-indices: graph-centrality + fan-in/out + observed-call-volume + observed-latency + observed-errors + hotpath + cycle-membership; not failure probabilities".into(),
            evidence_policy: "static-runtime-predicted-separated".into(),
            synthetic: false,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::DeepActivityAnalyzer;

    #[test]
    fn boundary_groups_workspace_roots_and_monorepo_packages() {
        assert_eq!(DeepActivityAnalyzer::boundary("services/auth/src/index.ts"), "services/auth");
        assert_eq!(DeepActivityAnalyzer::boundary("crates/core/src/lib.rs"), "crates/core");
        assert_eq!(DeepActivityAnalyzer::boundary("src/server.ts"), "src");
    }

    #[test]
    fn normalized_log_stays_bounded() {
        assert_eq!(DeepActivityAnalyzer::normalized_log(0.0, 10.0), 0.0);
        assert!((0.0..=1.0).contains(&DeepActivityAnalyzer::normalized_log(5.0, 10.0)));
        assert_eq!(DeepActivityAnalyzer::normalized_log(10.0, 10.0), 1.0);
    }
}
