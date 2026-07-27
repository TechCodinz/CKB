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

impl TestCoverageAnalyzer {
    /// Analyze coverage gaps between test files and production nodes in graph
    pub fn analyze_gaps(graph: &DependencyGraph) -> anyhow::Result<TestCoverageGapReport> {
        let nodes = graph.get_all_nodes();
        
        let mut test_nodes = Vec::new();
        let mut prod_nodes = Vec::new();

        for node in &nodes {
            if node.file_path.contains("test") || node.file_path.contains("spec") || node.name.starts_with("test_") {
                test_nodes.push(node);
            } else {
                prod_nodes.push(node);
            }
        }

        let mut covered_count = 0;
        let mut untested_hotpaths = Vec::new();

        for prod_node in &prod_nodes {
            let callers = graph.get_callers(&prod_node.id);
            let is_tested = callers.iter().any(|c| c.0.contains("test") || c.0.contains("spec"));

            if is_tested {
                covered_count += 1;
            } else {
                let callers_count = callers.len();
                let is_critical = callers_count >= 2 || prod_node.file_path.contains("core") || prod_node.file_path.contains("api");

                if is_critical {
                    let risk = ((callers_count as f32 * 0.25) + 0.3).min(0.98);
                    untested_hotpaths.push(UntestedHotpath {
                        function_name: prod_node.name.clone(),
                        file_path: prod_node.file_path.clone(),
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
