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
        // build_global() errors if a global pool already exists (e.g. this is
        // the second CkbEngine constructed in this process — plausible from
        // the Node/WASM bindings or any long-running host that creates more
        // than one engine). That's not fatal: the existing pool is still
        // perfectly usable, so we just ignore the "already initialized" case
        // instead of unwrap()-panicking the whole process over it.
        if let Err(e) = ThreadPoolBuilder::new().num_threads(num_cpus::get()).build_global() {
            tracing::debug!("Rayon global pool already initialized, reusing it: {}", e);
        }
        
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
    
    /// Reads this repo's own declared package/module name from whichever
    /// manifest file exists at the scan root. Best-effort: returns `None`
    /// rather than erroring if nothing recognizable is found, since not
    /// every scanned path is a package root (e.g. a subdirectory scan).
    fn detect_package_identity(path: &str) -> Option<String> {
        let root = std::path::Path::new(path);

        if let Ok(content) = std::fs::read_to_string(root.join("package.json")) {
            if let Ok(json) = serde_json::from_str::<serde_json::Value>(&content) {
                if let Some(name) = json.get("name").and_then(|n| n.as_str()) {
                    return Some(name.to_string());
                }
            }
        }

        if let Ok(content) = std::fs::read_to_string(root.join("Cargo.toml")) {
            for line in content.lines() {
                let line = line.trim();
                if let Some(rest) = line.strip_prefix("name") {
                    let rest = rest.trim_start();
                    if let Some(rest) = rest.strip_prefix('=') {
                        let name = rest.trim().trim_matches('"');
                        if !name.is_empty() {
                            return Some(name.to_string());
                        }
                    }
                }
                // Stop at the end of the [package] table (a naive but
                // sufficient parse — avoids picking up a dependency's `name`
                // field further down the file).
                if line.starts_with('[') && line != "[package]" {
                    break;
                }
            }
        }

        if let Ok(content) = std::fs::read_to_string(root.join("go.mod")) {
            for line in content.lines() {
                if let Some(module) = line.trim().strip_prefix("module ") {
                    return Some(module.trim().to_string());
                }
            }
        }

        if let Ok(content) = std::fs::read_to_string(root.join("pyproject.toml")) {
            for line in content.lines() {
                let line = line.trim();
                if let Some(rest) = line.strip_prefix("name") {
                    let rest = rest.trim_start();
                    if let Some(rest) = rest.strip_prefix('=') {
                        let name = rest.trim().trim_matches('"');
                        if !name.is_empty() {
                            return Some(name.to_string());
                        }
                    }
                }
            }
        }

        None
    }

    /// Collects the deduplicated set of external (non-relative) import
    /// sources referenced across every parsed file, normalized to just the
    /// package root — `"@myorg/pkg/deep/path"` becomes `"@myorg/pkg"`,
    /// `"lodash/debounce"` becomes `"lodash"` — since that's what a
    /// `package.json`/`Cargo.toml` "name" field would actually match against.
    fn collect_external_dependencies(file_analyses: &[FileAnalysis]) -> Vec<String> {
        let mut deps: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();

        for analysis in file_analyses {
            for import in &analysis.imports {
                let source = &import.source;
                let is_relative = source.starts_with('.') || source.starts_with('/');
                if is_relative || source.is_empty() {
                    continue;
                }

                let normalized = if let Some(stripped) = source.strip_prefix('@') {
                    // Scoped package: @scope/name/rest -> @scope/name
                    let mut parts = stripped.splitn(3, '/');
                    match (parts.next(), parts.next()) {
                        (Some(scope), Some(name)) => format!("@{}/{}", scope, name),
                        _ => source.clone(),
                    }
                } else {
                    // Unscoped: name/rest -> name
                    source.split('/').next().unwrap_or(source).to_string()
                };

                deps.insert(normalized);
            }
        }

        deps.into_iter().collect()
    }

    pub async fn scan_codebase(&self, path: &str) -> Result<ScanReport, anyhow::Error> {
        tracing::info!("Scanning codebase at {}", path);
        let started_at = std::time::Instant::now();
        
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
            duration_ms: started_at.elapsed().as_secs_f64() * 1000.0,
            package_identity: Self::detect_package_identity(path),
            external_dependencies: Self::collect_external_dependencies(&file_analyses),
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
    
    /// Session-level blast-radius aggregation.
    ///
    /// An AI coding agent (or a human) making many edits in one session
    /// produces one `analyze_impact` call per edit — nobody wants to read 20
    /// separate reports to understand "what did this session actually touch,
    /// and is any of it risky?" This runs impact analysis for every change in
    /// `changes`, then merges the results: dedupes affected nodes/files
    /// across all of them, surfaces the highest and average risk score, and
    /// flags which affected nodes have zero test coverage (cross-referenced
    /// against `TestCoverageAnalyzer`) — i.e. "you touched code nothing
    /// tests, in a way that could break other things."
    pub async fn analyze_session_impact(&self, changes: &[SessionChange]) -> Result<SessionImpactSummary, anyhow::Error> {
        let mut per_change = Vec::with_capacity(changes.len());
        for change in changes {
            let impact = self.analyze_impact(&change.file, change.line, change.change_type).await?;
            per_change.push(impact);
        }

        let mut unique_nodes: std::collections::HashSet<String> = std::collections::HashSet::new();
        let mut unique_files: std::collections::HashSet<String> = std::collections::HashSet::new();
        let mut risk_scores: Vec<f32> = Vec::new();

        for impact in &per_change {
            risk_scores.push(impact.risk_score);
            for node in impact.direct_impacts.iter().chain(impact.indirect_impacts.iter()) {
                unique_nodes.insert(node.node.0.clone());
                unique_files.insert(node.path.to_string_lossy().to_string());
            }
        }

        // Cross-reference affected nodes against real test coverage gaps
        // rather than guessing — reuses the same detector the
        // `/api/v1/test-gaps` route already exposes.
        let untested_affected: Vec<String> = {
            let graph = self.graph.read().await;
            match TestCoverageAnalyzer::analyze_gaps(&graph) {
                Ok(gap_report) => {
                    let untested_ids: std::collections::HashSet<String> = gap_report.untested_hotpaths.iter()
                        .map(|h| format!("{}::{}", h.file_path, h.function_name))
                        .collect();
                    unique_nodes.iter().filter(|n| untested_ids.contains(*n)).cloned().collect()
                }
                Err(_) => Vec::new(),
            }
        };

        let highest_risk_score = risk_scores.iter().cloned().fold(0.0_f32, f32::max);
        let average_risk_score = if risk_scores.is_empty() {
            0.0
        } else {
            risk_scores.iter().sum::<f32>() / risk_scores.len() as f32
        };

        Ok(SessionImpactSummary {
            changes_analyzed: changes.len(),
            unique_affected_nodes: unique_nodes.len(),
            unique_affected_files: unique_files.len(),
            affected_files: unique_files.into_iter().collect(),
            highest_risk_score,
            average_risk_score,
            untested_affected_nodes: untested_affected,
            per_change,
        })
    }

    /// Query architectural boundaries
    pub async fn get_boundaries(&self) -> Result<Vec<ArchitectureBoundary>, anyhow::Error> {
        let graph = self.graph.read().await;
        self.analyzer.infer_boundaries(&graph)
    }

    /// All nodes currently in the graph (from the last scan). Used by the
    /// natural-language Q&A feature to build retrieval context.
    pub async fn get_all_nodes(&self) -> Vec<Node> {
        let graph = self.graph.read().await;
        graph.get_all_nodes()
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
    ///
    /// The lower-level `detect_semantic_clones` below requires the caller to
    /// already have every file's contents in memory. Previously the only
    /// caller (the `ckb_detect_semantic_clones` MCP tool) passed an empty
    /// `HashMap`, so the feature always reported zero clones no matter what
    /// was actually in the repo. This variant does the file discovery + read
    /// itself, the same way `scan_codebase` does, so it's actually usable
    /// with just a path.
    pub async fn detect_semantic_clones_at(&self, path: &str) -> Result<CloneReport, anyhow::Error> {
        let files = self.discover_files(path)?;
        let mut file_contents = std::collections::HashMap::with_capacity(files.len());
        for file in files {
            if let Ok(content) = tokio::fs::read_to_string(&file).await {
                file_contents.insert(file, content);
            }
            // Files that fail to read (binary, permissions, races) are
            // silently skipped rather than failing the whole scan — same
            // tolerance the parser layer applies elsewhere.
        }
        Ok(CloneDetector::detect(&file_contents))
    }

    /// Lower-level variant for callers that already hold file contents in
    /// memory (e.g. tests, or an IDE extension with unsaved-buffer content).
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
        let started_at = std::time::Instant::now();
        
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
            duration_ms: started_at.elapsed().as_secs_f64() * 1000.0,
            package_identity: Self::detect_package_identity(path),
            // NOTE: incremental scans only parse `changed_files`, so this
            // only reflects imports from the files that changed in this
            // pass, not the whole repo. Fine for its actual use (fast
            // iterative rescans) but callers doing federation/cross-repo
            // analysis should use a full `scan_codebase` report instead.
            external_dependencies: Self::collect_external_dependencies(&file_analyses),
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
    /// Wall-clock time the scan took, in milliseconds. Actually measured (see
    /// `scan_codebase`/`scan_incremental`) — used downstream by
    /// `federation::IntelligenceBenchmarkMetrics` to report a real indexing
    /// speed instead of a hardcoded placeholder number.
    pub duration_ms: f64,
    /// This repo's own declared package/module name, if detectable from
    /// `package.json`/`Cargo.toml`/`go.mod`/`pyproject.toml` at the scan
    /// root. Used by `federation::FederatedGraphEngine` to do real
    /// cross-repo dependency matching (does repo A actually import a
    /// package published by repo B?) instead of a text-mention heuristic.
    pub package_identity: Option<String>,
    /// Deduplicated set of non-relative (external, published-package) import
    /// sources referenced anywhere in this repo — e.g. `"@myorg/shared-api"`,
    /// `"lodash"`. Relative imports (`"./foo"`, `"../bar"`) are excluded
    /// since they can never be a cross-repo reference by definition.
    pub external_dependencies: Vec<String>,
}

#[cfg(test)]
mod lib_tests {
    use super::*;
    use crate::parser::FileAnalysis;
    use crate::types::{Import, ImportKind};

    fn temp_dir_with_file(filename: &str, content: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("ckb_test_{}_{}", std::process::id(), filename.replace('.', "_")));
        std::fs::create_dir_all(&dir).expect("create temp dir");
        std::fs::write(dir.join(filename), content).expect("write temp manifest");
        dir
    }

    fn mock_file_analysis(imports: Vec<Import>) -> FileAnalysis {
        FileAnalysis {
            path: "src/lib.rs".to_string(),
            nodes: Vec::new(),
            imports,
            exports: Vec::new(),
            calls: Vec::new(),
            type_relations: Vec::new(),
        }
    }

    fn named_import(source: &str) -> Import {
        Import { source: source.to_string(), symbols: Vec::new(), kind: ImportKind::Named }
    }

    #[test]
    fn detects_package_identity_from_package_json() {
        let dir = temp_dir_with_file("package.json", r#"{"name": "@myorg/widget-service", "version": "1.0.0"}"#);
        let identity = CkbEngine::detect_package_identity(dir.to_str().unwrap());
        assert_eq!(identity, Some("@myorg/widget-service".to_string()));
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn detects_package_identity_from_cargo_toml() {
        let dir = temp_dir_with_file("Cargo.toml", "[package]\nname = \"ckb-core\"\nversion = \"0.1.0\"\n\n[dependencies]\nserde = \"1.0\"\n");
        let identity = CkbEngine::detect_package_identity(dir.to_str().unwrap());
        assert_eq!(identity, Some("ckb-core".to_string()));
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn detects_package_identity_from_go_mod() {
        let dir = temp_dir_with_file("go.mod", "module github.com/myorg/myservice\n\ngo 1.21\n");
        let identity = CkbEngine::detect_package_identity(dir.to_str().unwrap());
        assert_eq!(identity, Some("github.com/myorg/myservice".to_string()));
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn returns_none_when_no_manifest_present() {
        let dir = std::env::temp_dir().join(format!("ckb_test_empty_{}", std::process::id()));
        std::fs::create_dir_all(&dir).ok();
        let identity = CkbEngine::detect_package_identity(dir.to_str().unwrap());
        assert_eq!(identity, None);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn collects_deduplicated_external_dependencies_and_excludes_relative_imports() {
        let files = vec![
            mock_file_analysis(vec![
                named_import("lodash"),
                named_import("./local-helper"),   // relative — must be excluded
                named_import("../shared/utils"),  // relative — must be excluded
                named_import("@myorg/shared-api"),
            ]),
            mock_file_analysis(vec![
                named_import("lodash"), // duplicate of above — must be deduplicated
                named_import("react"),
            ]),
        ];

        let deps = CkbEngine::collect_external_dependencies(&files);

        assert!(deps.contains(&"lodash".to_string()));
        assert!(deps.contains(&"react".to_string()));
        assert!(deps.contains(&"@myorg/shared-api".to_string()));
        assert!(!deps.iter().any(|d| d.starts_with('.')), "relative imports must not appear as external dependencies");
        // lodash appeared twice across the two files but must only be counted once.
        assert_eq!(deps.iter().filter(|d| *d == "lodash").count(), 1);
    }

    #[test]
    fn normalizes_deep_and_scoped_import_paths_to_package_root() {
        let files = vec![mock_file_analysis(vec![
            named_import("lodash/debounce"),         // -> lodash
            named_import("@myorg/shared-api/client"), // -> @myorg/shared-api
        ])];

        let deps = CkbEngine::collect_external_dependencies(&files);

        assert!(deps.contains(&"lodash".to_string()));
        assert!(deps.contains(&"@myorg/shared-api".to_string()));
        assert!(!deps.iter().any(|d| d.contains("/debounce")));
        assert!(!deps.iter().any(|d| d.contains("/client")));
    }
}

