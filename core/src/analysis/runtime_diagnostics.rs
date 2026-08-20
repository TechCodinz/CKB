//! Evidence contracts for deeper CKB Software MRI diagnostics.
//!
//! These types deliberately keep source-line profiler evidence, exact trace
//! comparison, multi-service runtime observations and infrastructure/eBPF
//! observations separate. None of these layers is allowed to silently upgrade
//! another layer into stronger evidence.

use super::live_execution::{ObservedExecutionStep, ObservedExecutionTrace, RuntimeFlowType};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum LineEvidenceSource {
    Coverage,
    Profiler,
    RuntimeProbe,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ObservedLineExecution {
    pub file: String,
    pub line: u32,
    pub hits: u64,
    pub source: LineEvidenceSource,
    pub observed_start_unix_nano: u64,
    pub observed_end_unix_nano: u64,
    #[serde(default)]
    pub trace_ids: Vec<String>,
    #[serde(default)]
    pub function_id: Option<String>,
    #[serde(default)]
    pub evidence_ref: Option<String>,
    pub synthetic: bool,
}

impl ObservedLineExecution {
    pub fn is_valid_observation(&self) -> bool {
        !self.synthetic
            && !self.file.trim().is_empty()
            && self.line > 0
            && self.hits > 0
            && self.observed_start_unix_nano > 0
            && self.observed_end_unix_nano > self.observed_start_unix_nano
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct LineExecutionReport {
    pub observed_lines: Vec<ObservedLineExecution>,
    pub files_with_line_evidence: usize,
    pub total_observed_hits: u64,
    pub evidence_policy: String,
    pub synthetic: bool,
}

pub fn line_execution_report(lines: &[ObservedLineExecution]) -> LineExecutionReport {
    let mut valid: Vec<ObservedLineExecution> = lines
        .iter()
        .filter(|line| line.is_valid_observation())
        .cloned()
        .collect();
    valid.sort_by(|a, b| a.file.cmp(&b.file).then_with(|| a.line.cmp(&b.line)));
    let files: BTreeSet<String> = valid.iter().map(|line| line.file.clone()).collect();
    let total_observed_hits = valid.iter().map(|line| line.hits).sum();
    LineExecutionReport {
        observed_lines: valid,
        files_with_line_evidence: files.len(),
        total_observed_hits,
        evidence_policy: "line execution requires explicit coverage/profiler/runtime-probe evidence; function spans alone never establish executed source lines".to_string(),
        synthetic: false,
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct TraceDivergence {
    pub step_index: usize,
    #[serde(default)]
    pub success_step: Option<ObservedExecutionStep>,
    #[serde(default)]
    pub failed_step: Option<ObservedExecutionStep>,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct FailureTraceComparison {
    pub success_trace_id: String,
    pub failed_trace_id: String,
    pub common_prefix_steps: usize,
    #[serde(default)]
    pub first_divergence: Option<TraceDivergence>,
    pub success_measured_duration_ms: f64,
    pub failed_measured_duration_ms: f64,
    pub measured_duration_delta_ms: f64,
    pub success_error_steps: usize,
    pub failed_error_steps: usize,
    pub evidence_policy: String,
    pub synthetic: bool,
}

fn step_signature(step: &ObservedExecutionStep) -> (&str, &str, &str, RuntimeFlowType) {
    (
        step.source.0.as_str(),
        step.target.0.as_str(),
        step.operation.as_str(),
        step.flow_type,
    )
}

fn measured_trace_duration(trace: &ObservedExecutionTrace) -> f64 {
    let start = trace.steps.iter().map(|step| step.start_unix_nano).min();
    let end = trace.steps.iter().map(|step| step.end_unix_nano).max();
    match (start, end) {
        (Some(start), Some(end)) if end >= start => (end - start) as f64 / 1_000_000.0,
        _ => 0.0,
    }
}

pub fn compare_observed_traces(
    success: &ObservedExecutionTrace,
    failed: &ObservedExecutionTrace,
) -> FailureTraceComparison {
    let mut success_steps = success.steps.clone();
    let mut failed_steps = failed.steps.clone();
    success_steps.sort_by(|a, b| {
        a.start_unix_nano
            .cmp(&b.start_unix_nano)
            .then_with(|| a.span_id.cmp(&b.span_id))
    });
    failed_steps.sort_by(|a, b| {
        a.start_unix_nano
            .cmp(&b.start_unix_nano)
            .then_with(|| a.span_id.cmp(&b.span_id))
    });

    let limit = success_steps.len().max(failed_steps.len());
    let mut common_prefix_steps = 0usize;
    let mut first_divergence = None;
    for index in 0..limit {
        let left = success_steps.get(index);
        let right = failed_steps.get(index);
        let same = match (left, right) {
            (Some(left), Some(right)) => {
                step_signature(left) == step_signature(right) && left.error == right.error
            }
            _ => false,
        };
        if same {
            common_prefix_steps += 1;
            continue;
        }
        let reason = match (left, right) {
            (None, Some(_)) => "failed trace contains an additional observed step".to_string(),
            (Some(_), None) => "successful trace contains an additional observed step".to_string(),
            (Some(left), Some(right)) if step_signature(left) == step_signature(right) => {
                "observed error state diverged at the same runtime boundary".to_string()
            }
            (Some(_), Some(_)) => "observed runtime sequence diverged".to_string(),
            (None, None) => "no divergence".to_string(),
        };
        first_divergence = Some(TraceDivergence {
            step_index: index,
            success_step: left.cloned(),
            failed_step: right.cloned(),
            reason,
        });
        break;
    }

    let success_measured_duration_ms = measured_trace_duration(success);
    let failed_measured_duration_ms = measured_trace_duration(failed);
    FailureTraceComparison {
        success_trace_id: success.trace_id.clone(),
        failed_trace_id: failed.trace_id.clone(),
        common_prefix_steps,
        first_divergence,
        success_measured_duration_ms,
        failed_measured_duration_ms,
        measured_duration_delta_ms: failed_measured_duration_ms - success_measured_duration_ms,
        success_error_steps: success_steps.iter().filter(|step| step.error).count(),
        failed_error_steps: failed_steps.iter().filter(|step| step.error).count(),
        evidence_policy: "comparison uses exact observed step sequence/error state and measured timestamps only; it does not infer an uninstrumented root cause".to_string(),
        synthetic: false,
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ObservedServiceBoundary {
    pub trace_id: String,
    pub source_service: String,
    pub target_service: String,
    pub flow_type: RuntimeFlowType,
    pub duration_ms: f64,
    pub error: bool,
    pub observed_unix_nano: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ServiceRuntimeNode {
    pub service: String,
    pub observed_calls: u64,
    pub observed_errors: u64,
    pub observed_incoming_services: usize,
    pub observed_outgoing_services: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ServiceRuntimeEdge {
    pub source_service: String,
    pub target_service: String,
    pub flow_type: RuntimeFlowType,
    pub observed_calls: u64,
    pub observed_errors: u64,
    pub measured_total_duration_ms: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct MultiServiceRuntimeBody {
    pub services: Vec<ServiceRuntimeNode>,
    pub boundaries: Vec<ServiceRuntimeEdge>,
    pub evidence_policy: String,
    pub synthetic: bool,
}

pub fn multi_service_runtime_body(observations: &[ObservedServiceBoundary]) -> MultiServiceRuntimeBody {
    #[derive(Default)]
    struct NodeAccumulator {
        calls: u64,
        errors: u64,
        incoming: BTreeSet<String>,
        outgoing: BTreeSet<String>,
    }
    #[derive(Default)]
    struct EdgeAccumulator {
        calls: u64,
        errors: u64,
        total_duration_ms: f64,
    }

    let mut nodes: BTreeMap<String, NodeAccumulator> = BTreeMap::new();
    let mut edges: BTreeMap<(String, String, String), (RuntimeFlowType, EdgeAccumulator)> = BTreeMap::new();
    for observation in observations {
        let source = observation.source_service.trim();
        let target = observation.target_service.trim();
        if source.is_empty() || target.is_empty() || observation.trace_id.trim().is_empty() {
            continue;
        }
        let source_node = nodes.entry(source.to_string()).or_default();
        source_node.calls += 1;
        source_node.errors += u64::from(observation.error);
        source_node.outgoing.insert(target.to_string());
        let target_node = nodes.entry(target.to_string()).or_default();
        target_node.incoming.insert(source.to_string());

        let flow_key = format!("{:?}", observation.flow_type);
        let (_, edge) = edges
            .entry((source.to_string(), target.to_string(), flow_key))
            .or_insert_with(|| (observation.flow_type, EdgeAccumulator::default()));
        edge.calls += 1;
        edge.errors += u64::from(observation.error);
        edge.total_duration_ms += observation.duration_ms.max(0.0);
    }

    MultiServiceRuntimeBody {
        services: nodes
            .into_iter()
            .map(|(service, value)| ServiceRuntimeNode {
                service,
                observed_calls: value.calls,
                observed_errors: value.errors,
                observed_incoming_services: value.incoming.len(),
                observed_outgoing_services: value.outgoing.len(),
            })
            .collect(),
        boundaries: edges
            .into_iter()
            .map(|((source_service, target_service, _), (flow_type, value))| ServiceRuntimeEdge {
                source_service,
                target_service,
                flow_type,
                observed_calls: value.calls,
                observed_errors: value.errors,
                measured_total_duration_ms: value.total_duration_ms,
            })
            .collect(),
        evidence_policy: "service topology is constructed only from explicitly observed service identities and boundaries; source/package ownership is a separate resolution step".to_string(),
        synthetic: false,
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum InfrastructureEvidenceKind {
    Process,
    Container,
    Network,
    FileIo,
    Syscall,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct InfrastructureObservation {
    pub kind: InfrastructureEvidenceKind,
    pub source: String,
    pub subject: String,
    pub observed_start_unix_nano: u64,
    pub observed_end_unix_nano: u64,
    #[serde(default)]
    pub peer: Option<String>,
    #[serde(default)]
    pub measured_duration_ms: Option<f64>,
    #[serde(default)]
    pub source_identity: Option<String>,
    pub synthetic: bool,
}

impl InfrastructureObservation {
    pub fn is_valid_observation(&self) -> bool {
        !self.synthetic
            && self.source.eq_ignore_ascii_case("ebpf")
            && !self.subject.trim().is_empty()
            && self.observed_start_unix_nano > 0
            && self.observed_end_unix_nano >= self.observed_start_unix_nano
    }

    pub fn establishes_source_execution(&self) -> bool {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::NodeId;

    fn step(trace: &str, span: &str, source: &str, target: &str, start: u64, duration_ms: u64, error: bool) -> ObservedExecutionStep {
        ObservedExecutionStep {
            trace_id: trace.into(),
            span_id: span.into(),
            parent_span_id: "parent".into(),
            source: NodeId(source.into()),
            target: NodeId(target.into()),
            operation: target.into(),
            flow_type: RuntimeFlowType::Function,
            start_unix_nano: start,
            end_unix_nano: start + duration_ms * 1_000_000,
            duration_ms: duration_ms as f64,
            error,
            protocol: None,
            db_system: None,
            messaging_system: None,
        }
    }

    #[test]
    fn line_execution_requires_explicit_valid_observation() {
        let report = line_execution_report(&[
            ObservedLineExecution {
                file: "src/pay.rs".into(), line: 44, hits: 8, source: LineEvidenceSource::Profiler,
                observed_start_unix_nano: 10, observed_end_unix_nano: 20, trace_ids: vec!["t1".into()], function_id: None, evidence_ref: None, synthetic: false,
            },
            ObservedLineExecution {
                file: "src/fake.rs".into(), line: 1, hits: 1, source: LineEvidenceSource::RuntimeProbe,
                observed_start_unix_nano: 10, observed_end_unix_nano: 20, trace_ids: vec![], function_id: None, evidence_ref: None, synthetic: true,
            },
        ]);
        assert_eq!(report.observed_lines.len(), 1);
        assert_eq!(report.total_observed_hits, 8);
        assert!(!report.synthetic);
    }

    #[test]
    fn trace_compare_reports_first_observed_divergence_only() {
        let success = ObservedExecutionTrace {
            trace_id: "ok".into(), roots: vec![], steps: vec![
                step("ok", "1", "request", "controller", 1, 2, false),
                step("ok", "2", "controller", "service", 3_000_001, 4, false),
            ],
        };
        let failed = ObservedExecutionTrace {
            trace_id: "bad".into(), roots: vec![], steps: vec![
                step("bad", "1", "request", "controller", 1, 2, false),
                step("bad", "2", "controller", "database", 3_000_001, 9, true),
            ],
        };
        let comparison = compare_observed_traces(&success, &failed);
        assert_eq!(comparison.common_prefix_steps, 1);
        let divergence = comparison.first_divergence.unwrap();
        assert_eq!(divergence.step_index, 1);
        assert_eq!(divergence.reason, "observed runtime sequence diverged");
        assert_eq!(comparison.failed_error_steps, 1);
        assert!(!comparison.synthetic);
    }

    #[test]
    fn service_body_uses_only_explicit_service_boundaries() {
        let report = multi_service_runtime_body(&[
            ObservedServiceBoundary { trace_id: "t1".into(), source_service: "web".into(), target_service: "api".into(), flow_type: RuntimeFlowType::HttpClient, duration_ms: 4.0, error: false, observed_unix_nano: 10 },
            ObservedServiceBoundary { trace_id: "t2".into(), source_service: "api".into(), target_service: "db".into(), flow_type: RuntimeFlowType::Database, duration_ms: 8.0, error: true, observed_unix_nano: 20 },
            ObservedServiceBoundary { trace_id: "".into(), source_service: "invented".into(), target_service: "db".into(), flow_type: RuntimeFlowType::Database, duration_ms: 2.0, error: false, observed_unix_nano: 30 },
        ]);
        assert_eq!(report.boundaries.len(), 2);
        assert_eq!(report.services.len(), 3);
        assert!(!report.synthetic);
    }

    #[test]
    fn ebpf_observation_never_proves_source_execution_by_itself() {
        let observation = InfrastructureObservation {
            kind: InfrastructureEvidenceKind::Network,
            source: "ebpf".into(),
            subject: "pid:1234".into(),
            observed_start_unix_nano: 10,
            observed_end_unix_nano: 20,
            peer: Some("10.0.0.5:443".into()),
            measured_duration_ms: Some(1.2),
            source_identity: Some("src/server.rs::handle".into()),
            synthetic: false,
        };
        assert!(observation.is_valid_observation());
        assert!(!observation.establishes_source_execution());
    }
}
