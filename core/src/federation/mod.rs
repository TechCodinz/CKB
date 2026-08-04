//! Multi-Repo / Monorepo Federated Graph Module
//! Merges dependency graphs from multiple repositories into a unified cross-service knowledge graph

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use crate::types::*;
use crate::ScanReport;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FederatedRepoInfo {
    pub repo_name: String,
    pub repo_path: String,
    pub total_nodes: usize,
    pub total_edges: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CrossRepoEdge {
    pub source_repo: String,
    pub source_node: String,
    pub target_repo: String,
    pub target_node: String,
    pub edge_kind: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FederationReport {
    pub total_repos_federated: usize,
    pub repos: Vec<FederatedRepoInfo>,
    pub total_federated_nodes: usize,
    pub cross_repo_edges_count: usize,
    pub cross_repo_edges: Vec<CrossRepoEdge>,
}

pub struct FederatedGraphEngine;

impl FederatedGraphEngine {
    /// Merge multiple codebase reports into a single federated graph overview.
    ///
    /// Cross-repo edges are now detected two ways, in priority order:
    ///
    /// 1. **Real dependency matching** (new): if repo A's `external_dependencies`
    ///    (its actual parsed import statements, minus relative imports) contains
    ///    repo B's `package_identity` (B's own declared `package.json`/
    ///    `Cargo.toml`/`go.mod`/`pyproject.toml` name), that's a genuine,
    ///    verifiable cross-repo dependency — A really does import a package B
    ///    publishes. This requires both reports to have been produced by a
    ///    `ScanReport`-producing scan (not hand-constructed with those fields
    ///    empty).
    /// 2. **Text-evidence fallback**: if #1 finds nothing (e.g. `package_identity`
    ///    couldn't be detected for a repo, or the caller's reports predate these
    ///    fields), falls back to checking whether one repo's detected
    ///    patterns/violations literally mention the other repo's name — weaker,
    ///    but still evidence-based rather than fabricated.
    ///
    /// This still can't catch every real relationship (e.g. HTTP calls to a
    /// service with no shared package, or a private/unpublished internal
    /// package with a name that doesn't match how it's imported) — that would
    /// need contract-file matching (OpenAPI/protobuf) or config-driven service
    /// topology, which is a larger feature to build on top of this.
    pub fn federate(reports: &HashMap<String, ScanReport>) -> FederationReport {
        let mut total_nodes = 0;
        let mut total_edges = 0;
        let mut repos = Vec::new();
        let mut cross_edges = Vec::new();

        let repo_names: Vec<String> = reports.keys().cloned().collect();

        // package_identity -> repo_name, so we can look up "who owns this
        // import" in O(1) instead of O(repos) per dependency.
        let identity_to_repo: HashMap<String, String> = reports.iter()
            .filter_map(|(name, r)| r.package_identity.clone().map(|id| (id, name.clone())))
            .collect();

        for (repo_name, report) in reports {
            total_nodes += report.nodes;
            total_edges += report.edges;

            repos.push(FederatedRepoInfo {
                repo_name: repo_name.clone(),
                repo_path: format!("./repos/{}", repo_name),
                total_nodes: report.nodes,
                total_edges: report.edges,
            });

            let mut matched_repos: std::collections::HashSet<String> = std::collections::HashSet::new();

            // Priority 1: real import -> package_identity matches.
            for dep in &report.external_dependencies {
                if let Some(target_repo) = identity_to_repo.get(dep) {
                    if target_repo != repo_name && matched_repos.insert(target_repo.clone()) {
                        cross_edges.push(CrossRepoEdge {
                            source_repo: repo_name.clone(),
                            source_node: format!("{}::api", repo_name),
                            target_repo: target_repo.clone(),
                            target_node: format!("{}::client", target_repo),
                            edge_kind: "VerifiedPackageDependency".to_string(),
                        });
                    }
                }
            }

            // Priority 2: text-evidence fallback, only for repo pairs not
            // already confirmed above (avoids duplicate/weaker-evidence edges
            // for a relationship we've already verified for real).
            for other_repo in &repo_names {
                if other_repo == repo_name || matched_repos.contains(other_repo) {
                    continue;
                }
                let other_lower = other_repo.to_lowercase();

                let mentioned_in_patterns = report.patterns.iter().any(|p| {
                    p.description.to_lowercase().contains(&other_lower)
                        || p.boundaries.iter().any(|b| b.name.to_lowercase().contains(&other_lower))
                });
                let mentioned_in_drift = report.drift.iter().any(|d| {
                    d.message.to_lowercase().contains(&other_lower)
                        || d.boundary.to_lowercase().contains(&other_lower)
                        || d.from.0.to_lowercase().contains(&other_lower)
                        || d.to.0.to_lowercase().contains(&other_lower)
                });

                if mentioned_in_patterns || mentioned_in_drift {
                    cross_edges.push(CrossRepoEdge {
                        source_repo: repo_name.clone(),
                        source_node: format!("{}::api", repo_name),
                        target_repo: other_repo.clone(),
                        target_node: format!("{}::client", other_repo),
                        edge_kind: "TextEvidenceApiCall".to_string(),
                    });
                }
            }
        }

        FederationReport {
            total_repos_federated: reports.len(),
            repos,
            total_federated_nodes: total_nodes,
            cross_repo_edges_count: cross_edges.len(),
            cross_repo_edges: cross_edges,
        }
    }

    /// Analyze organization-wide architectural intelligence across federated repositories.
    pub fn analyze_org_intelligence(reports: &HashMap<String, ScanReport>) -> OrganizationalIntelligenceReport {
        let analysis_started_at = std::time::Instant::now();
        let fed = Self::federate(reports);

        let mut tech_debt_by_repo = Vec::new();
        let mut bottleneck_microservices = Vec::new();

        for (repo_name, report) in reports {
            // Technical debt is now derived from the ACTUAL violations this
            // repo's scan detected (report.drift), weighted by severity, not
            // the previous formula that fabricated a number purely from edge
            // count with no relationship to real detected problems.
            let weighted_severity: f64 = report.drift.iter().map(|v| match v.severity {
                Severity::Critical => 4.0,
                Severity::Error => 3.0,
                Severity::Warning => 2.0,
                Severity::Info => 1.0,
            }).sum();
            let raw_debt = if report.nodes > 0 {
                (weighted_severity / report.nodes as f64) * 100.0
            } else {
                0.0
            };
            let debt_percent = ((raw_debt.min(100.0)) * 10.0).round() / 10.0;

            let risk_level = if debt_percent > 15.0 {
                "High"
            } else if debt_percent > 5.0 {
                "Moderate"
            } else {
                "Low"
            }.to_string();

            tech_debt_by_repo.push(RepoTechDebtScore {
                repo_name: repo_name.clone(),
                technical_debt_percent: debt_percent,
                // Real count from the drift detector, not an estimate.
                architectural_violations: report.drift.len(),
                risk_level,
            });

            if report.nodes > 50 {
                bottleneck_microservices.push(repo_name.clone());
            }
        }

        tech_debt_by_repo.sort_by(|a, b| {
            b.technical_debt_percent
                .partial_cmp(&a.technical_debt_percent)
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        if bottleneck_microservices.is_empty() && !fed.repos.is_empty() {
            bottleneck_microservices.push(fed.repos[0].repo_name.clone());
        }

        OrganizationalIntelligenceReport {
            total_organization_repositories: fed.total_repos_federated,
            total_cross_service_dependencies: fed.cross_repo_edges_count,
            bottleneck_microservices,
            highest_technical_debt_repos: tech_debt_by_repo,
            benchmarks: IntelligenceBenchmarkMetrics::from_reports(reports, analysis_started_at.elapsed()),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RepoTechDebtScore {
    pub repo_name: String,
    pub technical_debt_percent: f64,
    pub architectural_violations: usize,
    pub risk_level: String,
}

/// Aggregate metrics about the federated scans themselves.
///
/// Previously this struct's `Default` impl hardcoded numbers like "14,200
/// files/sec" and "96.8% impact prediction accuracy" — constants that never
/// changed regardless of what was actually scanned, presented to the caller
/// (including the `/api/v1/org/analytics` REST endpoint) as if they were
/// real measurements. That's not a bug so much as fabricated data being
/// served as fact — genuinely risky for a paid product's customers to see as
/// "your organization's benchmarks." This version only reports numbers
/// computed from the actual scan reports passed in.
///
/// Fields that would require accumulated ground-truth (e.g. "false positive
/// rate" needs users confirming/dismissing violations over time; "impact
/// prediction accuracy" needs comparing predicted vs. actual blast radius
/// after a real change ships) are intentionally left out rather than
/// fabricated — they're a legitimate feature to build once that feedback
/// loop exists, not something a single scan can produce.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IntelligenceBenchmarkMetrics {
    /// Real: total files_processed across all federated repos.
    pub total_files_indexed: usize,
    /// Real: total drift violations detected across all federated repos.
    pub total_violations_detected: usize,
    /// Real: average files/sec across repos that reported a nonzero scan
    /// duration (duration_ms is measured with `std::time::Instant` in
    /// `CkbEngine::scan_codebase`/`scan_incremental`).
    pub avg_indexing_speed_files_per_sec: f64,
    /// Real: wall-clock time this federation/org-intelligence computation
    /// itself took to run, in milliseconds.
    pub analysis_duration_ms: f64,
}

impl IntelligenceBenchmarkMetrics {
    pub fn from_reports(reports: &HashMap<String, ScanReport>, analysis_elapsed: std::time::Duration) -> Self {
        let total_files_indexed: usize = reports.values().map(|r| r.files_processed).sum();
        let total_violations_detected: usize = reports.values().map(|r| r.drift.len()).sum();

        let speeds: Vec<f64> = reports.values()
            .filter(|r| r.duration_ms > 0.0)
            .map(|r| r.files_processed as f64 / (r.duration_ms / 1000.0))
            .collect();
        let avg_indexing_speed_files_per_sec = if speeds.is_empty() {
            0.0
        } else {
            speeds.iter().sum::<f64>() / speeds.len() as f64
        };

        Self {
            total_files_indexed,
            total_violations_detected,
            avg_indexing_speed_files_per_sec: (avg_indexing_speed_files_per_sec * 10.0).round() / 10.0,
            analysis_duration_ms: analysis_elapsed.as_secs_f64() * 1000.0,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrganizationalIntelligenceReport {
    pub total_organization_repositories: usize,
    pub total_cross_service_dependencies: usize,
    pub bottleneck_microservices: Vec<String>,
    pub highest_technical_debt_repos: Vec<RepoTechDebtScore>,
    pub benchmarks: IntelligenceBenchmarkMetrics,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mock_report(package_identity: Option<&str>, external_deps: &[&str]) -> ScanReport {
        ScanReport {
            files_processed: 1,
            nodes: 10,
            edges: 5,
            patterns: Vec::new(),
            drift: Vec::new(),
            snapshot_id: "test".to_string(),
            duration_ms: 100.0,
            package_identity: package_identity.map(|s| s.to_string()),
            external_dependencies: external_deps.iter().map(|s| s.to_string()).collect(),
        }
    }

    #[test]
    fn detects_real_cross_repo_edge_via_package_identity() {
        // repo-a genuinely imports the package repo-b publishes — this is
        // the real, verifiable signal (the fix for the audit's fabricated
        // "every repo pair gets an edge" bug).
        let mut reports = HashMap::new();
        reports.insert("repo-a".to_string(), mock_report(Some("@myorg/repo-a"), &["@myorg/repo-b", "lodash"]));
        reports.insert("repo-b".to_string(), mock_report(Some("@myorg/repo-b"), &[]));

        let fed = FederatedGraphEngine::federate(&reports);

        assert_eq!(fed.cross_repo_edges.len(), 1);
        let edge = &fed.cross_repo_edges[0];
        assert_eq!(edge.source_repo, "repo-a");
        assert_eq!(edge.target_repo, "repo-b");
        assert_eq!(edge.edge_kind, "VerifiedPackageDependency");
    }

    #[test]
    fn does_not_fabricate_edges_between_unrelated_repos() {
        // Neither repo imports the other, and neither's patterns/violations
        // mention the other by name — there should be ZERO edges. This is
        // the direct regression test for the original bug: the old code
        // unconditionally created an edge for every single repo pair.
        let mut reports = HashMap::new();
        reports.insert("repo-a".to_string(), mock_report(Some("@myorg/repo-a"), &["lodash", "react"]));
        reports.insert("repo-b".to_string(), mock_report(Some("@myorg/repo-b"), &["express"]));
        reports.insert("repo-c".to_string(), mock_report(Some("@myorg/repo-c"), &[]));

        let fed = FederatedGraphEngine::federate(&reports);

        assert_eq!(fed.cross_repo_edges.len(), 0, "unrelated repos must not get fabricated edges");
        assert_eq!(fed.total_repos_federated, 3);
    }

    #[test]
    fn ignores_self_reference() {
        // A repo whose external_dependencies happens to contain its OWN
        // package_identity (unusual, but possible with self-referential
        // path aliases) must not create a self-edge.
        let mut reports = HashMap::new();
        reports.insert("repo-a".to_string(), mock_report(Some("@myorg/repo-a"), &["@myorg/repo-a"]));

        let fed = FederatedGraphEngine::federate(&reports);
        assert_eq!(fed.cross_repo_edges.len(), 0);
    }

    #[test]
    fn intelligence_benchmarks_reflect_real_scan_data_not_fabricated_constants() {
        let mut reports = HashMap::new();
        reports.insert("repo-a".to_string(), mock_report(Some("@myorg/repo-a"), &[]));

        let metrics = IntelligenceBenchmarkMetrics::from_reports(&reports, std::time::Duration::from_millis(50));

        assert_eq!(metrics.total_files_indexed, 1);
        assert_eq!(metrics.total_violations_detected, 0);
        // duration_ms was 100.0 and files_processed was 1, so indexing speed
        // should be computed (10 files/sec), not a hardcoded placeholder.
        assert!((metrics.avg_indexing_speed_files_per_sec - 10.0).abs() < 0.01);
    }
}

