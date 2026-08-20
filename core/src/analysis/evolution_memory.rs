//! Evidence-first long-term architecture/runtime evolution memory.
//!
//! This module correlates explicit snapshots, deployments, runtime windows and
//! incidents by timestamp and commit identity. A temporal association is never
//! upgraded into a causal claim without a stronger evidence source.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ArchitectureSnapshotObservation {
    pub snapshot_id: String,
    pub observed_unix_nano: u64,
    #[serde(default)]
    pub commit_sha: Option<String>,
    pub node_count: u64,
    pub edge_count: u64,
    pub finding_count: u64,
    pub synthetic: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct DeploymentObservation {
    pub deployment_id: String,
    pub observed_unix_nano: u64,
    #[serde(default)]
    pub commit_sha: Option<String>,
    pub environment: String,
    pub success: bool,
    pub synthetic: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeWindowObservation {
    pub window_start_unix_nano: u64,
    pub window_end_unix_nano: u64,
    pub executions: u64,
    pub trace_count: u64,
    pub error_rate: f64,
    pub p95_latency_ms: f64,
    pub synthetic: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct IncidentObservation {
    pub incident_id: String,
    pub opened_unix_nano: u64,
    #[serde(default)]
    pub closed_unix_nano: Option<u64>,
    #[serde(default)]
    pub affected_identities: Vec<String>,
    pub synthetic: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeShift {
    pub deployment_id: String,
    pub environment: String,
    #[serde(default)]
    pub commit_sha: Option<String>,
    #[serde(default)]
    pub matching_snapshot_id: Option<String>,
    pub before_window_end_unix_nano: u64,
    pub after_window_start_unix_nano: u64,
    pub error_rate_delta: f64,
    pub p95_latency_delta_ms: f64,
    pub execution_delta: i64,
    pub evidence_policy: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct IncidentProximity {
    pub deployment_id: String,
    pub incident_id: String,
    pub elapsed_after_deployment_ms: f64,
    pub evidence_policy: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct EvolutionMemoryReport {
    pub valid_snapshots: usize,
    pub valid_deployments: usize,
    pub valid_runtime_windows: usize,
    pub valid_incidents: usize,
    pub runtime_shifts: Vec<RuntimeShift>,
    pub incident_proximity: Vec<IncidentProximity>,
    pub snapshots_by_commit: BTreeMap<String, Vec<String>>,
    pub evidence_policy: String,
    pub synthetic: bool,
}

fn valid_snapshot(value: &ArchitectureSnapshotObservation) -> bool {
    !value.synthetic && !value.snapshot_id.trim().is_empty() && value.observed_unix_nano > 0
}

fn valid_deployment(value: &DeploymentObservation) -> bool {
    !value.synthetic
        && !value.deployment_id.trim().is_empty()
        && !value.environment.trim().is_empty()
        && value.observed_unix_nano > 0
}

fn valid_runtime(value: &RuntimeWindowObservation) -> bool {
    !value.synthetic
        && value.window_start_unix_nano > 0
        && value.window_end_unix_nano > value.window_start_unix_nano
        && value.error_rate.is_finite()
        && value.error_rate >= 0.0
        && value.p95_latency_ms.is_finite()
        && value.p95_latency_ms >= 0.0
}

fn valid_incident(value: &IncidentObservation) -> bool {
    !value.synthetic && !value.incident_id.trim().is_empty() && value.opened_unix_nano > 0
}

fn delta_u64(after: u64, before: u64) -> i64 {
    let delta = after as i128 - before as i128;
    delta.clamp(i64::MIN as i128, i64::MAX as i128) as i64
}

pub fn build_evolution_memory(
    snapshots: &[ArchitectureSnapshotObservation],
    deployments: &[DeploymentObservation],
    runtime_windows: &[RuntimeWindowObservation],
    incidents: &[IncidentObservation],
    association_horizon_ms: u64,
) -> EvolutionMemoryReport {
    let valid_snapshots: Vec<_> = snapshots
        .iter()
        .filter(|item| valid_snapshot(item))
        .cloned()
        .collect();
    let mut valid_deployments: Vec<_> = deployments
        .iter()
        .filter(|item| valid_deployment(item))
        .cloned()
        .collect();
    let mut valid_runtime: Vec<_> = runtime_windows
        .iter()
        .filter(|item| valid_runtime(item))
        .cloned()
        .collect();
    let valid_incidents: Vec<_> = incidents
        .iter()
        .filter(|item| valid_incident(item))
        .cloned()
        .collect();

    valid_deployments.sort_by_key(|item| item.observed_unix_nano);
    valid_runtime.sort_by_key(|item| item.window_start_unix_nano);

    let mut snapshots_by_commit: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for snapshot in &valid_snapshots {
        if let Some(commit) = snapshot
            .commit_sha
            .as_ref()
            .filter(|value| !value.trim().is_empty())
        {
            snapshots_by_commit
                .entry(commit.clone())
                .or_default()
                .push(snapshot.snapshot_id.clone());
        }
    }

    let mut runtime_shifts = Vec::new();
    let mut incident_proximity = Vec::new();
    let horizon_nanos = association_horizon_ms.saturating_mul(1_000_000);

    for deployment in &valid_deployments {
        let before = valid_runtime
            .iter()
            .filter(|window| window.window_end_unix_nano <= deployment.observed_unix_nano)
            .max_by_key(|window| window.window_end_unix_nano);
        let after = valid_runtime
            .iter()
            .filter(|window| {
                window.window_start_unix_nano >= deployment.observed_unix_nano
                    && window
                        .window_start_unix_nano
                        .saturating_sub(deployment.observed_unix_nano)
                        <= horizon_nanos
            })
            .min_by_key(|window| window.window_start_unix_nano);

        if let (Some(before), Some(after)) = (before, after) {
            let matching_snapshot_id = deployment.commit_sha.as_ref().and_then(|commit| {
                snapshots_by_commit
                    .get(commit)
                    .and_then(|ids| ids.last())
                    .cloned()
            });
            runtime_shifts.push(RuntimeShift {
                deployment_id: deployment.deployment_id.clone(),
                environment: deployment.environment.clone(),
                commit_sha: deployment.commit_sha.clone(),
                matching_snapshot_id,
                before_window_end_unix_nano: before.window_end_unix_nano,
                after_window_start_unix_nano: after.window_start_unix_nano,
                error_rate_delta: after.error_rate - before.error_rate,
                p95_latency_delta_ms: after.p95_latency_ms - before.p95_latency_ms,
                execution_delta: delta_u64(after.executions, before.executions),
                evidence_policy: "measured before/after runtime windows are temporally associated with this deployment; this is not a causal attribution".into(),
            });
        }

        for incident in &valid_incidents {
            if incident.opened_unix_nano < deployment.observed_unix_nano {
                continue;
            }
            let elapsed = incident.opened_unix_nano - deployment.observed_unix_nano;
            if elapsed > horizon_nanos {
                continue;
            }
            incident_proximity.push(IncidentProximity {
                deployment_id: deployment.deployment_id.clone(),
                incident_id: incident.incident_id.clone(),
                elapsed_after_deployment_ms: elapsed as f64 / 1_000_000.0,
                evidence_policy: "incident timing is near this observed deployment; proximity alone does not establish that the deployment caused the incident".into(),
            });
        }
    }

    EvolutionMemoryReport {
        valid_snapshots: valid_snapshots.len(),
        valid_deployments: valid_deployments.len(),
        valid_runtime_windows: valid_runtime.len(),
        valid_incidents: valid_incidents.len(),
        runtime_shifts,
        incident_proximity,
        snapshots_by_commit,
        evidence_policy: "CKB evolution memory links explicit timestamps and commit identities only. Temporal association, runtime change and incident proximity remain evidence labels rather than causal claims.".into(),
        synthetic: false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn evolution_memory_quantifies_shift_without_claiming_cause() {
        let report = build_evolution_memory(
            &[ArchitectureSnapshotObservation {
                snapshot_id: "snap-2".into(),
                observed_unix_nano: 200,
                commit_sha: Some("abc".into()),
                node_count: 100,
                edge_count: 130,
                finding_count: 2,
                synthetic: false,
            }],
            &[DeploymentObservation {
                deployment_id: "deploy-2".into(),
                observed_unix_nano: 300_000_000,
                commit_sha: Some("abc".into()),
                environment: "production".into(),
                success: true,
                synthetic: false,
            }],
            &[
                RuntimeWindowObservation {
                    window_start_unix_nano: 100_000_000,
                    window_end_unix_nano: 250_000_000,
                    executions: 100,
                    trace_count: 20,
                    error_rate: 0.01,
                    p95_latency_ms: 120.0,
                    synthetic: false,
                },
                RuntimeWindowObservation {
                    window_start_unix_nano: 350_000_000,
                    window_end_unix_nano: 500_000_000,
                    executions: 140,
                    trace_count: 25,
                    error_rate: 0.03,
                    p95_latency_ms: 180.0,
                    synthetic: false,
                },
            ],
            &[IncidentObservation {
                incident_id: "incident-9".into(),
                opened_unix_nano: 400_000_000,
                closed_unix_nano: None,
                affected_identities: vec!["src/pay.rs::charge".into()],
                synthetic: false,
            }],
            1_000,
        );

        assert_eq!(report.runtime_shifts.len(), 1);
        let shift = &report.runtime_shifts[0];
        assert_eq!(shift.matching_snapshot_id.as_deref(), Some("snap-2"));
        assert!((shift.p95_latency_delta_ms - 60.0).abs() < f64::EPSILON);
        assert!((shift.error_rate_delta - 0.02).abs() < f64::EPSILON);
        assert_eq!(shift.execution_delta, 40);
        assert!(shift.evidence_policy.contains("not a causal"));
        assert_eq!(report.incident_proximity.len(), 1);
        assert!(report.incident_proximity[0]
            .evidence_policy
            .contains("does not establish"));
        assert!(!report.synthetic);
    }

    #[test]
    fn synthetic_and_invalid_observations_are_excluded() {
        let report = build_evolution_memory(
            &[ArchitectureSnapshotObservation {
                snapshot_id: "fake".into(),
                observed_unix_nano: 10,
                commit_sha: None,
                node_count: 1,
                edge_count: 1,
                finding_count: 0,
                synthetic: true,
            }],
            &[DeploymentObservation {
                deployment_id: "".into(),
                observed_unix_nano: 10,
                commit_sha: None,
                environment: "production".into(),
                success: true,
                synthetic: false,
            }],
            &[RuntimeWindowObservation {
                window_start_unix_nano: 20,
                window_end_unix_nano: 10,
                executions: 0,
                trace_count: 0,
                error_rate: 0.0,
                p95_latency_ms: 0.0,
                synthetic: false,
            }],
            &[IncidentObservation {
                incident_id: "fake".into(),
                opened_unix_nano: 10,
                closed_unix_nano: None,
                affected_identities: vec![],
                synthetic: true,
            }],
            60_000,
        );
        assert_eq!(report.valid_snapshots, 0);
        assert_eq!(report.valid_deployments, 0);
        assert_eq!(report.valid_runtime_windows, 0);
        assert_eq!(report.valid_incidents, 0);
        assert!(report.runtime_shifts.is_empty());
        assert!(report.incident_proximity.is_empty());
    }
}
