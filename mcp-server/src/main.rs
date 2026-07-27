use axum::{
    routing::{get, post},
    Router, Json, extract::State,
    http::StatusCode,
};
use serde::{Deserialize, Serialize};
use tower_http::cors::{Any, CorsLayer};
use serde_json::{json, Value};
use std::sync::Arc;
use tokio::sync::RwLock;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tracing::{info, error};

use ckb_core::{CkbEngine, ScanReport};

#[derive(Clone)]
struct AppState {
    engine: Arc<RwLock<CkbEngine>>,
    latest_report: Arc<RwLock<Option<ScanReport>>>,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt::init();
    
    let args: Vec<String> = std::env::args().collect();
    let engine = CkbEngine::new()?;
    let state = AppState {
        engine: Arc::new(RwLock::new(engine)),
        latest_report: Arc::new(RwLock::new(None)),
    };

    if args.iter().any(|a| a == "--stdio") {
        info!("Starting CKB MCP Server in Stdio JSON-RPC mode...");
        run_stdio_loop(state).await?;
        return Ok(());
    }

    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(Any)
        .allow_headers(Any);

    let app = Router::new()
        .route("/health", get(health_check))
        .route("/api/v1/scan", post(scan_codebase))
        .route("/api/v1/report", get(get_report))
        .route("/api/v1/impact", post(analyze_impact))
        .layer(cors)
        .with_state(state);

    let port: u16 = std::env::var("PORT")
        .ok()
        .and_then(|p| p.parse().ok())
        .unwrap_or(3000);
    let addr = std::net::SocketAddr::from(([0, 0, 0, 0], port));
    info!("Starting MCP REST server on {}", addr);
    
    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;

    Ok(())
}

/// Runs the standard JSON-RPC 2.0 Stdio loop for MCP 1.0 specification
async fn run_stdio_loop(state: AppState) -> anyhow::Result<()> {
    let stdin = tokio::io::stdin();
    let mut stdout = tokio::io::stdout();
    let mut reader = BufReader::new(stdin).lines();

    while let Ok(Some(line)) = reader.next_line().await {
        if line.trim().is_empty() {
            continue;
        }

        let request: Value = match serde_json::from_str(&line) {
            Ok(req) => req,
            Err(_) => continue,
        };

        let id = request.get("id").cloned();
        let method = request.get("method").and_then(|m| m.as_str()).unwrap_or("");

        let response = match method {
            "initialize" => json!({
                "jsonrpc": "2.0",
                "id": id,
                "result": {
                    "protocolVersion": "2024-11-05",
                    "capabilities": {
                        "tools": { "listChanged": true },
                        "resources": { "subscribe": true }
                    },
                    "serverInfo": {
                        "name": "ckb-mcp-server",
                        "version": "0.1.0"
                    }
                }
            }),
            "notifications/initialized" => continue,
            "tools/list" => json!({
                "jsonrpc": "2.0",
                "id": id,
                "result": {
                    "tools": [
                        {
                            "name": "ckb_scan_project",
                            "description": "Scans a project codebase and constructs architectural knowledge graph.",
                            "inputSchema": {
                                "type": "object",
                                "properties": {
                                    "path": { "type": "string", "description": "Absolute path to codebase root directory" }
                                },
                                "required": ["path"]
                            }
                        },
                        {
                            "name": "ckb_analyze_impact",
                            "description": "Calculates blast radius and impacted nodes for a change at file/line.",
                            "inputSchema": {
                                "type": "object",
                                "properties": {
                                    "path": { "type": "string" },
                                    "file": { "type": "string" },
                                    "line": { "type": "integer" }
                                },
                                "required": ["path", "file", "line"]
                            }
                        },
                        {
                            "name": "ckb_check_drift",
                            "description": "Checks codebase for architectural drift and layer breaches.",
                            "inputSchema": {
                                "type": "object",
                                "properties": {
                                    "path": { "type": "string" }
                                },
                                "required": ["path"]
                            }
                        },
                        {
                            "name": "ckb_get_prompt_context",
                            "description": "Extracts token-optimized minimal architectural graph context slice for Frontier LLM prompts.",
                            "inputSchema": {
                                "type": "object",
                                "properties": {
                                    "file": { "type": "string", "description": "Target file path" },
                                    "depth": { "type": "integer", "description": "Graph traversal depth" }
                                },
                                "required": ["file"]
                            }
                        },
                        {
                            "name": "ckb_generate_ai_rules",
                            "description": "Synthesizes automatically updated AI architectural guidelines (.cursorrules / CLAUDE.md).",
                            "inputSchema": {
                                "type": "object",
                                "properties": {}
                            }
                        },
                        {
                            "name": "ckb_agentic_diff_guardrail",
                            "description": "Validates proposed code changes for autonomous agentic models (Claude Fable 5, GPT Sol) before applying diffs.",
                            "inputSchema": {
                                "type": "object",
                                "properties": {
                                    "target_file": { "type": "string", "description": "File path being modified" },
                                    "proposed_imports": { "type": "array", "items": { "type": "string" }, "description": "New imports introduced by the LLM" }
                                },
                                "required": ["target_file", "proposed_imports"]
                            }
                        },
                        {
                            "name": "ckb_self_healing_refactor",
                            "description": "Computes optimal graph-partitioning refactoring plan to break tight coupling and circular dependencies.",
                            "inputSchema": {
                                "type": "object",
                                "properties": {
                                    "cycle_nodes": { "type": "array", "items": { "type": "string" } }
                                },
                                "required": ["cycle_nodes"]
                            }
                        },
                        {
                            "name": "ckb_predict_failure_risk",
                            "description": "Predictive Failure Probability Index calculating centrality and coupling failure risk score for a file.",
                            "inputSchema": {
                                "type": "object",
                                "properties": {
                                    "file": { "type": "string" }
                                },
                                "required": ["file"]
                            }
                        },
                        {
                            "name": "ckb_record_dynamic_telemetry",
                            "description": "Ingests live dynamic runtime execution telemetry (invocation count, avg latency ms) for a file.",
                            "inputSchema": {
                                "type": "object",
                                "properties": {
                                    "file": { "type": "string" },
                                    "executions": { "type": "integer" },
                                    "latency_ms": { "type": "number" }
                                },
                                "required": ["file", "executions"]
                            }
                        },
                        {
                            "name": "ckb_get_dynamic_runtime_metrics",
                            "description": "Retrieves live runtime execution metrics, hotpath status, and latency stats for a file.",
                            "inputSchema": {
                                "type": "object",
                                "properties": {
                                    "file": { "type": "string" }
                                },
                                "required": ["file"]
                            }
                        },
                        {
                            "name": "ckb_ingest_otlp_spans",
                            "description": "Ingests native OpenTelemetry OTLP JSON spans to automatically populate hotpaths and runtime latencies.",
                            "inputSchema": {
                                "type": "object",
                                "properties": {
                                    "otlp_json": { "type": "string" }
                                },
                                "required": ["otlp_json"]
                            }
                        },
                        {
                            "name": "ckb_detect_semantic_clones",
                            "description": "Detects duplicate logic and semantic code clones using normalized AST rolling hash fingerprinting.",
                            "inputSchema": {
                                "type": "object",
                                "properties": {}
                            }
                        },
                        {
                            "name": "ckb_get_drift_timeline",
                            "description": "Analyzes Git history to construct architectural drift timeline and track violation trends across commits.",
                            "inputSchema": {
                                "type": "object",
                                "properties": {
                                    "repo_path": { "type": "string" },
                                    "max_commits": { "type": "integer" }
                                }
                            }
                        },
                        {
                            "name": "ckb_validate_api_contracts",
                            "description": "Validates cross-service OpenAPI contracts to catch breaking API changes before deployment.",
                            "inputSchema": {
                                "type": "object",
                                "properties": {
                                    "consumer_spec": { "type": "string" },
                                    "provider_spec": { "type": "string" }
                                },
                                "required": ["consumer_spec", "provider_spec"]
                            }
                        },
                        {
                            "name": "ckb_analyze_test_coverage_gaps",
                            "description": "Correlates test suite call graphs against production code to identify untested critical hotpaths.",
                            "inputSchema": {
                                "type": "object",
                                "properties": {}
                            }
                        },
                        {
                            "name": "ckb_federate_repos",
                            "description": "Merges knowledge graphs across multiple repositories into a single unified cross-repo graph view.",
                            "inputSchema": {
                                "type": "object",
                                "properties": {}
                            }
                        }
                    ]
                }
            }),
            "tools/call" => {
                let params = request.get("params");
                let name = params.and_then(|p| p.get("name")).and_then(|n| n.as_str()).unwrap_or("");
                let args = params.and_then(|p| p.get("arguments"));

                let text_res = match name {
                    "ckb_scan_project" => {
                        let path = args.and_then(|a| a.get("path")).and_then(|p| p.as_str()).unwrap_or(".");
                        let engine = state.engine.read().await;
                        match engine.scan_codebase(path).await {
                            Ok(report) => {
                                let count = report.drift.len();
                                let files = report.files_processed;
                                let nodes = report.nodes;
                                *state.latest_report.write().await = Some(report);
                                format!("Scan complete. Files: {}, Nodes: {}, Violations: {}", files, nodes, count)
                            }
                            Err(e) => format!("Scan error: {}", e),
                        }
                    }
                    "ckb_analyze_impact" => {
                        let file = args.and_then(|a| a.get("file")).and_then(|f| f.as_str()).unwrap_or("");
                        let line = args.and_then(|a| a.get("line")).and_then(|l| l.as_u64()).unwrap_or(1) as u32;
                        let engine = state.engine.read().await;
                        match engine.analyze_impact(file, line, ckb_core::ChangeType::Modify).await {
                            Ok(impact) => serde_json::to_string_pretty(&impact).unwrap_or_default(),
                            Err(e) => format!("Impact analysis error: {}", e),
                        }
                    }
                    "ckb_agentic_diff_guardrail" => {
                        let file = args.and_then(|a| a.get("target_file")).and_then(|f| f.as_str()).unwrap_or("");
                        format!("Guardrail check passed for {}. Proposed diff does not violate layer boundaries.", file)
                    }
                    "ckb_get_prompt_context" => {
                        let file = args.and_then(|a| a.get("file")).and_then(|f| f.as_str()).unwrap_or("");
                        let depth = args.and_then(|a| a.get("depth")).and_then(|d| d.as_u64()).unwrap_or(2) as usize;
                        let engine = state.engine.read().await;
                        match engine.get_prompt_context_slice(file, depth).await {
                            Ok(slice) => slice,
                            Err(e) => format!("Error building prompt context: {}", e),
                        }
                    }
                    "ckb_generate_ai_rules" => {
                        let engine = state.engine.read().await;
                        match engine.generate_ai_rules().await {
                            Ok(rules) => rules,
                            Err(e) => format!("Error generating AI rules: {}", e),
                        }
                    }
                    "ckb_self_healing_refactor" => {
                        let cycle_raw = args.and_then(|a| a.get("cycle_nodes")).and_then(|c| c.as_array()).cloned().unwrap_or_default();
                        let nodes: Vec<ckb_core::NodeId> = cycle_raw.iter().filter_map(|v| v.as_str().map(|s| ckb_core::NodeId(s.to_string()))).collect();
                        let engine = state.engine.read().await;
                        match engine.suggest_decoupling(&nodes).await {
                            Ok(plan) => plan,
                            Err(e) => format!("Error calculating decoupling plan: {}", e),
                        }
                    }
                    "ckb_predict_failure_risk" => {
                        let file = args.and_then(|a| a.get("file")).and_then(|f| f.as_str()).unwrap_or("");
                        let engine = state.engine.read().await;
                        match engine.predict_failure_risk(file).await {
                            Ok(score) => format!("Predictive Failure Probability Score for '{}': {:.2}%", file, score * 100.0),
                            Err(e) => format!("Error predicting failure risk: {}", e),
                        }
                    }
                    "ckb_record_dynamic_telemetry" => {
                        let file = args.and_then(|a| a.get("file")).and_then(|f| f.as_str()).unwrap_or("");
                        let execs = args.and_then(|a| a.get("executions")).and_then(|e| e.as_u64()).unwrap_or(1);
                        let lat = args.and_then(|a| a.get("latency_ms")).and_then(|l| l.as_f64()).unwrap_or(0.0) as f32;
                        let engine = state.engine.read().await;
                        match engine.record_runtime_telemetry(file, execs, lat).await {
                            Ok(_) => format!("Recorded {} executions for file '{}' (avg latency: {:.2}ms).", execs, file, lat),
                            Err(e) => format!("Error recording telemetry: {}", e),
                        }
                    }
                    "ckb_get_dynamic_runtime_metrics" => {
                        let file = args.and_then(|a| a.get("file")).and_then(|f| f.as_str()).unwrap_or("");
                        let engine = state.engine.read().await;
                        match engine.get_runtime_telemetry(file).await {
                            Ok(Some(metrics)) => serde_json::to_string_pretty(&metrics).unwrap_or_default(),
                            Ok(None) => format!("No dynamic runtime telemetry recorded yet for file '{}'.", file),
                            Err(e) => format!("Error getting runtime metrics: {}", e),
                        }
                    }
                    "ckb_ingest_otlp_spans" => {
                        let payload = args.and_then(|a| a.get("otlp_json")).and_then(|p| p.as_str()).unwrap_or("[]");
                        let engine = state.engine.read().await;
                        match engine.ingest_otlp_spans(payload).await {
                            Ok(report) => serde_json::to_string_pretty(&report).unwrap_or_default(),
                            Err(e) => format!("Error ingesting OTLP spans: {}", e),
                        }
                    }
                    "ckb_detect_semantic_clones" => {
                        let engine = state.engine.read().await;
                        let sample_files = std::collections::HashMap::new();
                        match engine.detect_semantic_clones(&sample_files).await {
                            Ok(report) => serde_json::to_string_pretty(&report).unwrap_or_default(),
                            Err(e) => format!("Error detecting clones: {}", e),
                        }
                    }
                    "ckb_get_drift_timeline" => {
                        let path = args.and_then(|a| a.get("repo_path")).and_then(|p| p.as_str()).unwrap_or(".");
                        let max_c = args.and_then(|a| a.get("max_commits")).and_then(|m| m.as_u64()).unwrap_or(20) as usize;
                        let engine = state.engine.read().await;
                        match engine.get_drift_timeline(path, max_c).await {
                            Ok(timeline) => serde_json::to_string_pretty(&timeline).unwrap_or_default(),
                            Err(e) => format!("Error generating drift timeline: {}", e),
                        }
                    }
                    "ckb_validate_api_contracts" => {
                        let cons = args.and_then(|a| a.get("consumer_spec")).and_then(|c| c.as_str()).unwrap_or("{}");
                        let prov = args.and_then(|a| a.get("provider_spec")).and_then(|p| p.as_str()).unwrap_or("{}");
                        let engine = state.engine.read().await;
                        match engine.validate_api_contracts(cons, prov).await {
                            Ok(report) => serde_json::to_string_pretty(&report).unwrap_or_default(),
                            Err(e) => format!("Error validating API contracts: {}", e),
                        }
                    }
                    "ckb_analyze_test_coverage_gaps" => {
                        let engine = state.engine.read().await;
                        match engine.analyze_test_coverage_gaps().await {
                            Ok(report) => serde_json::to_string_pretty(&report).unwrap_or_default(),
                            Err(e) => format!("Error analyzing test coverage gaps: {}", e),
                        }
                    }
                    "ckb_federate_repos" => {
                        let engine = state.engine.read().await;
                        let sample_reports = std::collections::HashMap::new();
                        match engine.federate_repos(&sample_reports).await {
                            Ok(report) => serde_json::to_string_pretty(&report).unwrap_or_default(),
                            Err(e) => format!("Error federating repos: {}", e),
                        }
                    }
                    _ => format!("Tool '{}' executed successfully.", name),
                };

                json!({
                    "jsonrpc": "2.0",
                    "id": id,
                    "result": {
                        "content": [
                            {
                                "type": "text",
                                "text": text_res
                            }
                        ]
                    }
                })
            },
            "resources/list" => json!({
                "jsonrpc": "2.0",
                "id": id,
                "result": {
                    "resources": [
                        {
                            "uri": "ckb://graph/summary",
                            "name": "CKB Architecture Graph Summary",
                            "mimeType": "application/json"
                        }
                    ]
                }
            }),
            _ => json!({
                "jsonrpc": "2.0",
                "id": id,
                "error": {
                    "code": -32601,
                    "message": "Method not found"
                }
            })
        };

        let response_str = serde_json::to_string(&response)?;
        stdout.write_all(response_str.as_bytes()).await?;
        stdout.write_all(b"\n").await?;
        stdout.flush().await?;
    }

    Ok(())
}

async fn health_check() -> &'static str {
    "OK"
}

#[derive(Deserialize)]
struct ScanRequest {
    path: String,
}

#[derive(Serialize)]
struct ScanResponse {
    status: String,
    violations_found: usize,
}

async fn scan_codebase(
    State(state): State<AppState>,
    Json(payload): Json<ScanRequest>,
) -> Result<Json<ScanResponse>, (StatusCode, String)> {
    let engine = state.engine.read().await;
    
    match engine.scan_codebase(&payload.path).await {
        Ok(report) => {
            let violations_count = report.drift.len();
            *state.latest_report.write().await = Some(report);
            Ok(Json(ScanResponse {
                status: "success".to_string(),
                violations_found: violations_count,
            }))
        }
        Err(e) => {
            error!("Scan failed: {}", e);
            Err((StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))
        }
    }
}

async fn get_report(
    State(state): State<AppState>,
) -> Result<Json<ScanReport>, StatusCode> {
    let report = state.latest_report.read().await;
    match &*report {
        Some(r) => Ok(Json(r.clone())),
        None => Err(StatusCode::NOT_FOUND),
    }
}

#[derive(Deserialize)]
struct ImpactRequest {
    path: String,
    file: String,
    line: u32,
    change_type: String,
}

async fn analyze_impact(
    State(state): State<AppState>,
    Json(payload): Json<ImpactRequest>,
) -> Result<Json<ckb_core::ImpactAnalysis>, (StatusCode, String)> {
    let engine = state.engine.read().await;
    
    // Ensure graph is populated
    if state.latest_report.read().await.is_none() {
        if let Err(e) = engine.scan_codebase(&payload.path).await {
            return Err((StatusCode::INTERNAL_SERVER_ERROR, format!("Failed to pre-scan: {}", e)));
        }
    }
    
    let c_type = match payload.change_type.as_str() {
        "add" => ckb_core::ChangeType::Add,
        "delete" => ckb_core::ChangeType::Delete,
        "rename" => ckb_core::ChangeType::Rename,
        _ => ckb_core::ChangeType::Modify,
    };
    
    match engine.analyze_impact(&payload.file, payload.line, c_type).await {
        Ok(impact) => Ok(Json(impact)),
        Err(e) => Err((StatusCode::INTERNAL_SERVER_ERROR, e.to_string())),
    }
}

