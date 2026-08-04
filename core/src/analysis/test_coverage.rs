//! AI Test Coverage Gap Analysis
//! Correlates test call graphs against production code to identify untested hotpaths and critical gaps

use serde::{Deserialize, Serialize};
use crate::graph::DependencyGraph;
use crate::types::*;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UntestedHotpath {
    pub function_name: String,
    pub file_path: String,
    pub line_number: u32,
    pub incoming_callers_count: usize,
    pub failure_risk_score: f32,
    pub suggested_test_name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TestCoverageGapReport {
    pub total_production_nodes: usize,
    pub total_test_nodes: usize,
    pub covered_nodes_count: usize,
    pub coverage_percentage: f32,
    pub untested_hotpaths: Vec<UntestedHotpath>,
    pub high_priority_gaps_count: usize,
}

pub struct TestCoverageAnalyzer;

/// `path_str.contains("test")` (the previous check) also matches `latest.rs`,
/// `contest.py`, `attestation.go`, `protestor.ts`, etc. — any production file
/// with "test" or "spec" as a mere substring got silently misclassified as a
/// test file, or treated as "covered" if a false-positive-matching file called
/// into it, hiding real gaps from the report. This checks path *segments* and
/// filename conventions instead of raw substrings.
fn is_test_path(path: &str) -> bool {
    let normalized = path.replace('\\', "/");
    let segments: Vec<&str> = normalized.split('/').collect();

    let dir_is_test = segments.iter().any(|s| {
        matches!(*s, "test" | "tests" | "spec" | "specs" | "__tests__" | "__test__")
    });
    if dir_is_test {
        return true;
    }

    let filename = segments.last().copied().unwrap_or("");
    let stem = filename.split('.').next().unwrap_or(filename);
    stem.starts_with("test_")
        || stem.ends_with("_test")
        || stem.ends_with("Test")
        || stem.starts_with("spec_")
        || stem.ends_with("_spec")
        || stem.ends_with(".test")
        || stem.ends_with(".spec")
        || filename.contains(".test.")
        || filename.contains(".spec.")
}

impl TestCoverageAnalyzer {
    /// Analyze coverage gaps between test files and production nodes in graph
    pub fn analyze_gaps(graph: &DependencyGraph) -> anyhow::Result<TestCoverageGapReport> {
        let nodes = graph.get_all_nodes();
        
        let mut test_nodes = Vec::new();
        let mut prod_nodes = Vec::new();

        for node in &nodes {
            let path_str = node.path.to_string_lossy();
            if is_test_path(&path_str) || node.name.starts_with("test_") {
                test_nodes.push(node);
            } else {
                prod_nodes.push(node);
            }
        }

        let mut covered_count = 0;
        let mut untested_hotpaths = Vec::new();

        for prod_node in &prod_nodes {
            let callers = graph.get_callers(&prod_node.id);
            let is_tested = callers.iter().any(|c| is_test_path(&c.0));

            if is_tested {
                covered_count += 1;
            } else {
                let callers_count = callers.len();
                let path_str = prod_node.path.to_string_lossy();
                let is_critical = callers_count >= 2 || path_str.contains("core") || path_str.contains("api");

                if is_critical {
                    let risk = ((callers_count as f32 * 0.25) + 0.3).min(0.98);
                    untested_hotpaths.push(UntestedHotpath {
                        function_name: prod_node.name.clone(),
                        file_path: path_str.into_owned(),
                        line_number: prod_node.line,
                        incoming_callers_count: callers_count,
                        failure_risk_score: risk,
                        suggested_test_name: format!("test_{}", prod_node.name),
                    });
                }
            }
        }

        let total_prod = prod_nodes.len();
        let coverage_pct = if total_prod > 0 {
            (covered_count as f32 / total_prod as f32) * 100.0
        } else {
            100.0
        };

        let high_priority_count = untested_hotpaths.iter().filter(|h| h.failure_risk_score > 0.6).count();

        Ok(TestCoverageGapReport {
            total_production_nodes: total_prod,
            total_test_nodes: test_nodes.len(),
            covered_nodes_count: covered_count,
            coverage_percentage: coverage_pct,
            untested_hotpaths,
            high_priority_gaps_count: high_priority_count,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Regression tests for the bug this function was rewritten to fix: the
    // original `path.contains("test")` check misclassified any file with
    // "test"/"spec" as a mere substring — not a path segment — as a test
    // file, silently hiding real coverage gaps. These lock in the fix.

    #[test]
    fn recognizes_real_test_files() {
        assert!(is_test_path("src/tests/helpers.rs"));
        assert!(is_test_path("src/__tests__/component.tsx"));
        assert!(is_test_path("lib/test/utils.py"));
        assert!(is_test_path("src/test_utils.py"));
        assert!(is_test_path("src/utils_test.go"));
        assert!(is_test_path("src/UtilsTest.java"));
        assert!(is_test_path("src/component.test.tsx"));
        assert!(is_test_path("src/component.spec.ts"));
        assert!(is_test_path("spec/models/user_spec.rb"));
    }

    #[test]
    fn does_not_misclassify_production_files_with_test_as_a_substring() {
        // This is the exact regression: these all contain "test" as a raw
        // substring but are ordinary production files, not test files.
        assert!(!is_test_path("src/latest.rs"));
        assert!(!is_test_path("src/latest_handler.rs"));
        assert!(!is_test_path("src/contest.py"));
        assert!(!is_test_path("src/attestation.go"));
        assert!(!is_test_path("src/protestor.ts"));
        assert!(!is_test_path("src/detestable.rs"));
        assert!(!is_test_path("src/contest_score.py"));
    }

    #[test]
    fn handles_windows_style_paths() {
        assert!(is_test_path("src\\tests\\helpers.rs"));
        assert!(!is_test_path("src\\latest.rs"));
    }
}
