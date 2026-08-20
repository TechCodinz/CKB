//! Evidence-first runtime analysis for the CKB Live Execution Twin.
//!
//! This module deliberately operates on exact observed span instances and
//! already-aggregated runtime metrics. It never invents missing runtime edges,
//! never upgrades an unresolved runtime identity into a source identity, and
//! never claims line-level execution unless a future profiler supplies explicit
//! line evidence.

use crate::types::{NodeId, RuntimeMetrics};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

pub const UNRESOLVED_RUNTIME_PREFIX: &str = "runtime-unresolved/";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum RuntimeFlowType {
    Function,
    HttpServer,
    HttpClient,
    Database,
    Cache,
    Queue,
    Event,
    Websocket,
    Unknown,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ObservedExecutionStep {
    pub trace_id: String,
    pub span_id: String,
    pub parent_span_id: String,
    pub source: NodeId,
    pub target: NodeId,
    pub operation: String,
    pub flow_type: RuntimeFlowType,
    pub start_unix_nano: u64,
    pub end_unix_nano: u64,
    pub duration_ms: f64,
    pub error: bool,
    #[serde(default)]
    pub protocol: Option<String>,
    #[serde(default)]
    pub db_system: Option<String>,
    #[serde(default)]
    pub messaging_system: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ObservedExecutionTrace {
    pub trace_id: String,
    #[serde(default)]
    pub roots: Vec<NodeId>,
    pub steps: Vec<ObservedExecutionStep>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum RuntimeSignal {
    Observed,
    Hot,
    Slow,
    Unstable,
    HighFanOut,
    CriticalPath,
    DeadCodeCandidate,
    UnresolvedIdentity,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeNodeInsight {
    pub node_id: NodeId,
    pub signals: Vec<RuntimeSignal>,
    pub execution_count: u64,
    pub avg_latency_ms: f32,
    pub error_rate: f32,
    pub observed_outgoing_targets: usize,
    pub observed_incoming_sources: usize,
    pub runtime_only_identity: bool,
    pub evidence: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeCausalNeighborhood {
    pub trace_id: String,
    pub selected_node: NodeId,
    pub observed_before: Vec<ObservedExecutionStep>,
    pub selected_edges: Vec<ObservedExecutionStep>,
    pub observed_after: Vec<ObservedExecutionStep>,
    pub complete_for_trace: bool,
    pub synthetic: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ObservationWindow {
    pub start_unix_nano: u64,
    pub end_unix_nano: u64,
}

impl ObservationWindow {
    pub fn is_valid(&self) -> bool {
        self.start_unix_nano > 0 && self.end_unix_nano > self.start_unix_nano
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeFusionReport {
    pub node_insights: Vec<RuntimeNodeInsight>,
    pub dead_code_candidates: Vec<NodeId>,
    pub observed_nodes: usize,
    pub unresolved_runtime_identities: usize,
    pub observation_window: Option<ObservationWindow>,
    pub evidence_policy: String,
    pub synthetic: bool,
}

#[derive(Debug, Clone, Copy)]
pub struct LiveExecutionThresholds {
    pub hot_execution_count: u64,
    pub slow_avg_latency_ms: f32,
    pub unstable_error_rate: f32,
    pub high_fan_out: usize,
}

impl Default for LiveExecutionThresholds {
    fn default() -> Self {
        Self {
            hot_execution_count: 500,
            slow_avg_latency_ms: 500.0,
            unstable_error_rate: 0.05,
            high_fan_out: 6,
        }
    }
}

pub struct LiveExecutionAnalyzer {
    thresholds: LiveExecutionThresholds,
}

impl Default for LiveExecutionAnalyzer {
    fn default() -> Self {
        Self::new(LiveExecutionThresholds::default())
    }
}

impl LiveExecutionAnalyzer {
    pub fn new(thresholds: LiveExecutionThresholds) -> Self {
        Self { thresholds }
    }

    pub fn is_unresolved_runtime_identity(node_id: &NodeId) -> bool {
        node_id.0.starts_with(UNRESOLVED_RUNTIME_PREFIX)
    }

    /// Build a runtime/static fusion report without inventing runtime evidence.
    ///
    /// `static_nodes` is the set of identities discovered from source analysis.
    /// `runtime` contains only measured runtime metrics. `traces` contains exact
    /// parent/child span instances. A static node is considered a dead-code
    /// *candidate* only when the caller supplies an explicit valid observation
    /// window and the node has no runtime observation in either metrics or
    /// exact traces. This is deliberately weaker than claiming the code is dead.
    pub fn fuse(
        &self,
        static_nodes: &HashSet<NodeId>,
        runtime: &HashMap<NodeId, RuntimeMetrics>,
        traces: &[ObservedExecutionTrace],
        observation_window: Option<ObservationWindow>,
    ) -> RuntimeFusionReport {
        let mut outgoing: HashMap<NodeId, HashSet<NodeId>> = HashMap::new();
        let mut incoming: HashMap<NodeId, HashSet<NodeId>> = HashMap::new();
        let mut traced_nodes = HashSet::new();

        for trace in traces {
            for root in &trace.roots {
                traced_nodes.insert(root.clone());
            }
            for step in &trace.steps {
                traced_nodes.insert(step.source.clone());
                traced_nodes.insert(step.target.clone());
                outgoing.entry(step.source.clone()).or_default().insert(step.target.clone());
                incoming.entry(step.target.clone()).or_default().insert(step.source.clone());
            }
        }

        let mut all_runtime_ids: HashSet<NodeId> = runtime.keys().cloned().collect();
        all_runtime_ids.extend(traced_nodes.iter().cloned());

        let mut node_insights = Vec::new();
        let mut unresolved_runtime_identities = 0usize;

        let mut ordered_runtime_ids: Vec<NodeId> = all_runtime_ids.into_iter().collect();
        ordered_runtime_ids.sort_by(|a, b| a.0.cmp(&b.0));

        for node_id in ordered_runtime_ids {
            let metrics = runtime.get(&node_id);
            let execution_count = metrics.map(|m| m.execution_count).unwrap_or(0);
            let avg_latency_ms = metrics.map(|m| m.avg_latency_ms).unwrap_or(0.0);
            let error_rate = metrics.map(|m| m.error_rate).unwrap_or(0.0);
            let out_count = outgoing.get(&node_id).map(HashSet::len).unwrap_or(0);
            let in_count = incoming.get(&node_id).map(HashSet::len).unwrap_or(0);
            let runtime_only_identity = Self::is_unresolved_runtime_identity(&node_id)
                || !static_nodes.contains(&node_id);

            let mut signals = vec![RuntimeSignal::Observed];
            if execution_count >= self.thresholds.hot_execution_count || metrics.map(|m| m.is_hotpath).unwrap_or(false) {
                signals.push(RuntimeSignal::Hot);
            }
            if avg_latency_ms >= self.thresholds.slow_avg_latency_ms {
                signals.push(RuntimeSignal::Slow);
            }
            if error_rate >= self.thresholds.unstable_error_rate {
                signals.push(RuntimeSignal::Unstable);
            }
            if out_count >= self.thresholds.high_fan_out {
                signals.push(RuntimeSignal::HighFanOut);
            }
            if runtime_only_identity {
                signals.push(RuntimeSignal::UnresolvedIdentity);
                unresolved_runtime_identities += 1;
            }

            let mut evidence = vec!["observed-runtime".to_string()];
            if metrics.is_some() {
                evidence.push("runtime-metrics".to_string());
            }
            if traced_nodes.contains(&node_id) {
                evidence.push("exact-span-instance".to_string());
            }
            if static_nodes.contains(&node_id) {
                evidence.push("source-identity-resolved".to_string());
            } else {
                evidence.push("source-identity-not-established".to_string());
            }

            node_insights.push(RuntimeNodeInsight {
                node_id,
                signals,
                execution_count,
                avg_latency_ms,
                error_rate,
                observed_outgoing_targets: out_count,
                observed_incoming_sources: in_count,
                runtime_only_identity,
                evidence,
            });
        }

        let can_assess_absence = observation_window.as_ref().is_some_and(ObservationWindow::is_valid);
        let mut dead_code_candidates = Vec::new();
        if can_assess_absence {
            for node_id in static_nodes {
                if !runtime.contains_key(node_id) && !traced_nodes.contains(node_id) {
                    dead_code_candidates.push(node_id.clone());
                }
            }
            dead_code_candidates.sort_by(|a, b| a.0.cmp(&b.0));
            for dead in &dead_code_candidates {
                node_insights.push(RuntimeNodeInsight {
                    node_id: dead.clone(),
                    signals: vec![RuntimeSignal::DeadCodeCandidate],
                    execution_count: 0,
                    avg_latency_ms: 0.0,
                    error_rate: 0.0,
                    observed_outgoing_targets: 0,
                    observed_incoming_sources: 0,
                    runtime_only_identity: false,
                    evidence: vec![
                        "source-identity-resolved".to_string(),
                        "no-observation-in-explicit-window".to_string(),
                        "candidate-not-proof-of-dead-code".to_string(),
                    ],
                });
            }
        }

        node_insights.sort_by(|a, b| a.node_id.0.cmp(&b.node_id.0));
        RuntimeFusionReport {
            observed_nodes: node_insights.iter().filter(|node| node.signals.contains(&RuntimeSignal::Observed)).count(),
            node_insights,
            dead_code_candidates,
            unresolved_runtime_identities,
            observation_window,
            evidence_policy: "static-runtime-predicted-separated; runtime edges require exact observed evidence; absence is only a candidate inside an explicit observation window".to_string(),
            synthetic: false,
        }
    }

    /// Reconstruct only the causal neighborhood that exists in one exact trace.
    /// Missing parents/children remain missing. The result never splices paths
    /// from different trace ids or fills gaps using the static graph.
    pub fn causal_neighborhood(
        &self,
        trace: &ObservedExecutionTrace,
        selected_node: &NodeId,
    ) -> RuntimeCausalNeighborhood {
        let mut observed_before = Vec::new();
        let mut selected_edges = Vec::new();
        let mut observed_after = Vec::new();
        let mut selected_seen = false;

        let mut ordered = trace.steps.clone();
        ordered.sort_by(|a, b| {
            a.start_unix_nano
                .cmp(&b.start_unix_nano)
                .then_with(|| a.span_id.cmp(&b.span_id))
        });

        for step in ordered {
            let touches = &step.source == selected_node || &step.target == selected_node;
            if touches {
                selected_seen = true;
                selected_edges.push(step.clone());
                continue;
            }
            if selected_seen {
                observed_after.push(step);
            } else {
                observed_before.push(step);
            }
        }

        if !selected_seen {
            observed_before.clear();
            observed_after.clear();
        }

        RuntimeCausalNeighborhood {
            trace_id: trace.trace_id.clone(),
            selected_node: selected_node.clone(),
            observed_before,
            selected_edges,
            observed_after,
            complete_for_trace: selected_seen,
            synthetic: false,
        }
    }

    /// Select observed critical-path steps by measured duration. This is not a
    /// prediction: it ranks only exact steps that are already present in the
    /// supplied trace and returns enough steps to account for `coverage` of the
    /// trace's total measured step duration.
    pub fn observed_critical_path(
        &self,
        trace: &ObservedExecutionTrace,
        coverage: f64,
    ) -> Vec<ObservedExecutionStep> {
        let desired = coverage.clamp(0.0, 1.0);
        if desired == 0.0 || trace.steps.is_empty() {
            return Vec::new();
        }
        let total: f64 = trace.steps.iter().map(|step| step.duration_ms.max(0.0)).sum();
        if total <= f64::EPSILON {
            return Vec::new();
        }
        let target = total * desired;
        let mut ranked = trace.steps.clone();
        ranked.sort_by(|a, b| {
            b.duration_ms
                .partial_cmp(&a.duration_ms)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        let mut selected = Vec::new();
        let mut accumulated = 0.0;
        for step in ranked {
            accumulated += step.duration_ms.max(0.0);
            selected.push(step);
            if accumulated >= target {
                break;
            }
        }
        selected
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn node(value: &str) -> NodeId {
        NodeId(value.to_string())
    }

    fn step(trace: &str, span: &str, parent: &str, source: &str, target: &str, duration: f64) -> ObservedExecutionStep {
        ObservedExecutionStep {
            trace_id: trace.into(),
            span_id: span.into(),
            parent_span_id: parent.into(),
            source: node(source),
            target: node(target),
            operation: target.into(),
            flow_type: RuntimeFlowType::Function,
            start_unix_nano: 1,
            end_unix_nano: 1 + (duration * 1_000_000.0) as u64,
            duration_ms: duration,
            error: false,
            protocol: None,
            db_system: None,
            messaging_system: None,
        }
    }

    #[test]
    fn unresolved_identity_remains_runtime_only() {
        let analyzer = LiveExecutionAnalyzer::default();
        let unresolved = node("runtime-unresolved/function/work");
        let mut runtime = HashMap::new();
        runtime.insert(unresolved.clone(), RuntimeMetrics {
            execution_count: 9,
            avg_latency_ms: 4.0,
            error_rate: 0.0,
            is_hotpath: false,
        });
        let report = analyzer.fuse(&HashSet::new(), &runtime, &[], None);
        assert_eq!(report.unresolved_runtime_identities, 1);
        let insight = report.node_insights.iter().find(|item| item.node_id == unresolved).unwrap();
        assert!(insight.runtime_only_identity);
        assert!(insight.signals.contains(&RuntimeSignal::UnresolvedIdentity));
        assert!(!insight.evidence.contains(&"source-identity-resolved".to_string()));
    }

    #[test]
    fn heat_signals_use_only_supplied_runtime_metrics() {
        let analyzer = LiveExecutionAnalyzer::default();
        let id = node("src/checkout.ts::checkout");
        let static_nodes = HashSet::from([id.clone()]);
        let mut runtime = HashMap::new();
        runtime.insert(id.clone(), RuntimeMetrics {
            execution_count: 900,
            avg_latency_ms: 650.0,
            error_rate: 0.08,
            is_hotpath: true,
        });
        let report = analyzer.fuse(&static_nodes, &runtime, &[], None);
        let insight = report.node_insights.iter().find(|item| item.node_id == id).unwrap();
        assert!(insight.signals.contains(&RuntimeSignal::Hot));
        assert!(insight.signals.contains(&RuntimeSignal::Slow));
        assert!(insight.signals.contains(&RuntimeSignal::Unstable));
        assert!(!report.synthetic);
    }

    #[test]
    fn dead_code_candidate_requires_explicit_valid_window() {
        let analyzer = LiveExecutionAnalyzer::default();
        let id = node("src/unused.ts::unused");
        let static_nodes = HashSet::from([id.clone()]);

        let without_window = analyzer.fuse(&static_nodes, &HashMap::new(), &[], None);
        assert!(without_window.dead_code_candidates.is_empty());

        let invalid_window = analyzer.fuse(
            &static_nodes,
            &HashMap::new(),
            &[],
            Some(ObservationWindow { start_unix_nano: 10, end_unix_nano: 10 }),
        );
        assert!(invalid_window.dead_code_candidates.is_empty());

        let with_window = analyzer.fuse(
            &static_nodes,
            &HashMap::new(),
            &[],
            Some(ObservationWindow { start_unix_nano: 10, end_unix_nano: 20 }),
        );
        assert_eq!(with_window.dead_code_candidates, vec![id]);
    }

    #[test]
    fn causal_neighborhood_never_invents_missing_edges() {
        let analyzer = LiveExecutionAnalyzer::default();
        let trace = ObservedExecutionTrace {
            trace_id: "trace-1".into(),
            roots: vec![node("request")],
            steps: vec![
                step("trace-1", "2", "1", "request", "controller", 4.0),
                step("trace-1", "3", "2", "controller", "service", 8.0),
                step("trace-1", "4", "3", "service", "database", 15.0),
            ],
        };
        let result = analyzer.causal_neighborhood(&trace, &node("service"));
        assert!(result.complete_for_trace);
        assert_eq!(result.selected_edges.len(), 2);
        assert!(result.observed_before.iter().all(|edge| edge.trace_id == "trace-1"));
        assert!(result.observed_after.iter().all(|edge| edge.trace_id == "trace-1"));
        assert!(!result.synthetic);

        let missing = analyzer.causal_neighborhood(&trace, &node("not-observed"));
        assert!(!missing.complete_for_trace);
        assert!(missing.observed_before.is_empty());
        assert!(missing.selected_edges.is_empty());
        assert!(missing.observed_after.is_empty());
    }

    #[test]
    fn observed_critical_path_returns_only_real_steps() {
        let analyzer = LiveExecutionAnalyzer::default();
        let trace = ObservedExecutionTrace {
            trace_id: "trace-2".into(),
            roots: vec![],
            steps: vec![
                step("trace-2", "a", "root", "a", "b", 10.0),
                step("trace-2", "b", "a", "b", "c", 70.0),
                step("trace-2", "c", "b", "c", "d", 20.0),
            ],
        };
        let critical = analyzer.observed_critical_path(&trace, 0.70);
        assert_eq!(critical.len(), 1);
        assert_eq!(critical[0].span_id, "b");
    }
}
