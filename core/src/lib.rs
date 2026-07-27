//! CKB Core - Architectural intelligence engine
//!
//! This crate provides the core functionality for parsing code,
//! building dependency graphs, detecting architectural drift,
//! and analyzing impact of changes.

mod parser;
mod graph;
mod analysis;
mod storage;
mod error;
mod types;
pub mod telemetry;
pub mod contracts;
pub mod vcs;
pub mod federation;

pub use parser::*;
pub use graph::*;
pub use analysis::*;
pub use storage::*;
pub use error::*;
pub use types::*;
pub use telemetry::*;
pub use contracts::*;
pub use vcs::*;
pub use federation::*;

use std::sync::Arc;
use rayon::ThreadPoolBuilder;

/// CKB Engine - Main entry point for all functionality
pub struct CkbEngine {
    parser: Arc<parser::LanguageParser>,
    graph: Arc<tokio::sync::RwLock<graph::DependencyGraph>>,
    analyzer: Arc<analysis::ArchitectureAnalyzer>,
    storage: Arc<storage::GraphStorage>,
}

impl CkbEngine {
    /// Create a new CKB engine with default configuration
    pub fn new() -> Result<Self, anyhow::Error> {
        // Initialize rayon thread pool for parallel parsing
        ThreadPoolBuilder::new()
            .num_threads(num_cpus::get())
            .build_global()
            .unwrap();
        
        let parser = Arc::new(parser::LanguageParser::new());
        let storage = Arc::new(storage::GraphStorage::new("./ckb_data")?);
        let graph = Arc::new(tokio::sync::RwLock::new(graph::DependencyGraph::new()));
        let analyzer = Arc::new(analysis::ArchitectureAnalyzer::new());
        
        Ok(Self {
            parser,
            graph,
            analyzer,
            storage,
        })
    }
    
    /// Scan a codebase and build its knowledge graph
    pub async fn scan_codebase(&self, path: &str) -> Result<ScanReport, anyhow::Error> {
        tracing::info!("Scanning codebase at {}", path);
        
        // Find all source files
        let files = self.discover_files(path)?;
        
        // Parse files in parallel
        let mut tasks = Vec::new();
        for file in files {
            let parser = self.parser.clone();
            tasks.push(tokio::spawn(async move {
                parser.parse_file(&file).await
            }));
        }
        
        // Collect results
        let mut file_analyses = Vec::new();
        for task in tasks {
            if let Ok(analysis) = task.await? {
                file_analyses.push(analysis);
            }
        }
        
        // Build graph
        let mut graph = self.graph.write().await;
        for analysis in &file_analyses {
            graph.add_file(analysis)?;
        }
        
        // Build call graph and type graph
        graph.build_call_graph()?;
        graph.build_type_graph()?;
        
        // Detect architectural patterns
        let patterns = self.analyzer.detect_patterns(&graph)?;
        
        // Detect drift
        let drift = self.analyzer.detect_drift(&graph, &patterns)?;
        
        // Store snapshot
        let snapshot_id = self.storage.store_snapshot(&graph).await?;
        
        Ok(ScanReport {
            files_processed: file_analyses.len(),
            nodes: graph.node_count(),
            edges: graph.edge_count(),
            patterns,
            drift,
            snapshot_id,
        })
    }
    
    /// Analyze impact of a potential change
    pub async fn analyze_impact(&self, 
                               file: &str, 
                               line: u32,
                               change_type: ChangeType) -> Result<ImpactAnalysis, anyhow::Error> {
        let graph = self.graph.read().await;
        
        // Find affected nodes
        let affected = graph.find_affected_nodes(file, line)?;
        
        // Calculate impact propagation
        let impact = graph.calculate_impact(&affected, change_type)?;
        
        Ok(impact)
    }
    
    /// Query architectural boundaries
    pub async fn get_boundaries(&self) -> Result<Vec<ArchitectureBoundary>, anyhow::Error> {
        let graph = self.graph.read().await;
        self.analyzer.infer_boundaries(&graph)
    }

    /// Extract minimal token-optimized subgraph context slice for Frontier LLM prompts
    pub async fn get_prompt_context_slice(&self, file: &str, depth: usize) -> Result<String, anyhow::Error> {
        let graph = self.graph.read().await;
        self.analyzer.slice_context_for_prompt(&graph, file, depth)
    }

    /// Synthesize automatic AI system rules (.cursorrules / CLAUDE.md)
    pub async fn generate_ai_rules(&self) -> Result<String, anyhow::Error> {
        let graph = self.graph.read().await;
        self.analyzer.generate_ai_guidelines(&graph)
    }

    /// Self-Healing Refactoring Engine: Predicts optimal decoupling plan for architectural cycles
    pub async fn suggest_decoupling(&self, cycle_nodes: &[NodeId]) -> Result<String, anyhow::Error> {
        let graph = self.graph.read().await;
        self.analyzer.suggest_decoupling_refactor(&graph, cycle_nodes)
    }

    /// Predictive Failure Probability Index: Computes risk score for target file
    pub async fn predict_failure_risk(&self, file: &str) -> Result<f32, anyhow::Error> {
        let graph = self.graph.read().await;
        self.analyzer.predict_failure_probability(&graph, file)
    }

    /// Record live dynamic runtime trace execution telemetry for a node
    pub async fn record_runtime_telemetry(&self, file: &str, executions: u64, latency_ms: f32) -> Result<(), anyhow::Error> {
        let node_id = NodeId(format!("{}::file", file));
        let mut graph = self.graph.write().await;
        graph.record_runtime_trace(node_id, executions, latency_ms);
        Ok(())
    }

    /// Get live dynamic runtime metrics for a node
    pub async fn get_runtime_telemetry(&self, file: &str) -> Result<Option<RuntimeMetrics>, anyhow::Error> {
        let node_id = NodeId(format!("{}::file", file));
        let graph = self.graph.read().await;
        Ok(graph.get_runtime_metrics(&node_id).cloned())
    }

    /// Feature 1: Native OpenTelemetry OTLP span ingestion
    pub async fn ingest_otlp_spans(&self, raw_payload: &str) -> Result<OtlpIngestReport, anyhow::Error> {
        let metrics_map = OtlpReceiver::ingest_spans(raw_payload)?;
        let mut graph = self.graph.write().await;
        for (node_id, metrics) in &metrics_map {
            graph.record_runtime_trace(node_id.clone(), metrics.execution_count, metrics.avg_latency_ms);
        }
        Ok(OtlpReceiver::summarize(&metrics_map))
    }

    /// Feature 2: Semantic Clone & Duplicate Logic Detector
    pub async fn detect_semantic_clones(&self, file_contents: &std::collections::HashMap<String, String>) -> Result<CloneReport, anyhow::Error> {
        Ok(CloneDetector::detect(file_contents))
    }

    /// Feature 3: Git History Architectural Drift Timeline
    pub async fn get_drift_timeline(&self, repo_path: &str, max_commits: usize) -> Result<DriftTimeline, anyhow::Error> {
        GitDriftAnalyzer::build_timeline(repo_path, max_commits)
    }

    /// Feature 4: Cross-Service API Contract Validator
    pub async fn validate_api_contracts(&self, consumer_spec: &str, provider_spec: &str) -> Result<ContractValidationReport, anyhow::Error> {
        let consumer_eps = ApiContractValidator::parse_openapi_spec("consumer-service", consumer_spec)?;
        let provider_eps = ApiContractValidator::parse_openapi_spec("provider-service", provider_spec)?;
        Ok(ApiContractValidator::validate(&consumer_eps, &provider_eps))
    }

    /// Feature 5: AI Test Coverage Gap Analysis
    pub async fn analyze_test_coverage_gaps(&self) -> Result<TestCoverageGapReport, anyhow::Error> {
        let graph = self.graph.read().await;
        TestCoverageAnalyzer::analyze_gaps(&graph)
    }

    /// Feature 6: Multi-Repo / Monorepo Federated Graph Engine
    pub async fn federate_repos(&self, repo_reports: &std::collections::HashMap<String, ScanReport>) -> Result<FederationReport, anyhow::Error> {
        Ok(FederatedGraphEngine::federate(repo_reports))
    }
    
    /// Incrementally scan modified files in a codebase and update the graph
    pub async fn scan_incremental(&self, path: &str, changed_files: &[String]) -> Result<ScanReport, anyhow::Error> {
        tracing::info!("Incrementally scanning {} files in codebase at {}", changed_files.len(), path);
        
        let mut file_analyses = Vec::new();
        for file in changed_files {
            if let Ok(analysis) = self.parser.parse_file(file).await {
                file_analyses.push(analysis);
            }
        }
        
        let mut graph = self.graph.write().await;
        for analysis in &file_analyses {
            graph.add_file(analysis)?;
        }
        
        graph.build_call_graph()?;
        graph.build_type_graph()?;
        
        let patterns = self.analyzer.detect_patterns(&graph)?;
        let drift = self.analyzer.detect_drift(&graph, &patterns)?;
        let snapshot_id = self.storage.store_snapshot(&graph).await?;
        
        Ok(ScanReport {
            files_processed: file_analyses.len(),
            nodes: graph.node_count(),
            edges: graph.edge_count(),
            patterns,
            drift,
            snapshot_id,
        })
    }

    fn discover_files(&self, path: &str) -> Result<Vec<String>, anyhow::Error> {
        let mut files = Vec::new();
        let walker = ignore::Walk::new(path);
        
        for entry in walker {
            let entry = entry?;
            let path = entry.path();
            
            if path.is_file() {
                if let Some(ext) = path.extension() {
                    if self.parser.is_supported_extension(ext) {
                        files.push(path.to_string_lossy().to_string());
                    }
                }
            }
        }
        
        Ok(files)
    }
}

/// Report from a codebase scan
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ScanReport {
    pub files_processed: usize,
    pub nodes: usize,
    pub edges: usize,
    pub patterns: Vec<ArchitecturalPattern>,
    pub drift: Vec<DriftViolation>,
    pub snapshot_id: String,
}

