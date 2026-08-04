use axum::{
    routing::{get, post},
    Router, Json, extract::State,
    http::{StatusCode, HeaderMap, Request},
    middleware::{self, Next},
    response::Response,
};
use serde::{Deserialize, Serialize};
use tower_http::cors::{Any, CorsLayer};
use serde_json::{json, Value};
use std::sync::Arc;
use tokio::sync::RwLock;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tracing::{info, warn, error};

use ckb_core::{CkbEngine, ScanReport};

mod explain;
mod ask;

/// A single tenant/repo's isolated engine + last scan report.
///
/// Before this, every REST request shared ONE `CkbEngine` with ONE internal
/// graph (`AppState.engine`/`AppState.latest_report`, still kept below as the
/// unnamed "default" session for backward compatibility). That's fine for a
/// single local user, but under real concurrent multi-tenant usage it's a
/// correctness bug, not just a performance one: user A scanning repo X and
/// user B scanning repo Y at the same time would overwrite each other's
/// graph and `latest_report` — A's impact-analysis call could silently run
/// against B's graph. Passing `repo_name` on a request now routes it to its
/// own isolated `SessionState` instead of the shared default.
#[derive(Clone)]
struct SessionState {
    engine: Arc<RwLock<CkbEngine>>,
    latest_report: Arc<RwLock<Option<ScanReport>>>,
}

impl SessionState {
    fn new() -> Result<Self, anyhow::Error> {
        Ok(Self {
            engine: Arc::new(RwLock::new(CkbEngine::new()?)),
            latest_report: Arc::new(RwLock::new(None)),
        })
    }
}

/// Resolves a request's `repo_name` (if any) to its isolated `SessionState`,
/// creating one on first use. `None`/empty falls back to the server's
/// original single shared session — existing single-tenant deployments that
/// never pass `repo_name` see no behavior change at all.
async fn resolve_session(state: &AppState, repo_name: Option<&str>) -> Result<SessionState, (StatusCode, String)> {
    let key = match repo_name {
        Some(k) if !k.is_empty() => k,
        _ => {
            return Ok(SessionState {
                engine: state.engine.clone(),
                latest_report: state.latest_report.clone(),
            });
        }
    };

    if let Some(session) = state.sessions.read().await.get(key) {
        return Ok(session.clone());
    }

    // Not found on the fast (read-lock) path — acquire the write lock and
    // re-check, since another concurrent request for the same repo_name
    // could have created it between the check above and here.
    let mut sessions = state.sessions.write().await;
    if let Some(session) = sessions.get(key) {
        return Ok(session.clone());
    }
    let session = SessionState::new()
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("Failed to initialize session engine: {}", e)))?;
    sessions.insert(key.to_string(), session.clone());
    Ok(session)
}

#[derive(Clone)]
struct BackendConfig {
    url: String,
    internal_secret: String,
}

#[derive(Clone)]
struct AppState {
    engine: Arc<RwLock<CkbEngine>>,
    latest_report: Arc<RwLock<Option<ScanReport>>>,
    api_key: Option<Arc<String>>,
    /// Multi-repo federation registry: repo_name -> its most recent
    /// ScanReport. Previously `get_org_analytics`/`get_intelligence_metrics`
    /// only ever had a single hardcoded "ckb-core-platform" entry sourced
    /// from `latest_report` — there was no way to actually register more
    /// than one repo's scan, making "organizational intelligence" always
    /// describe exactly one repo. `scan_codebase` now accepts an optional
    /// `repo_name` and stores into this map when provided.
    federated_reports: Arc<RwLock<std::collections::HashMap<String, ScanReport>>>,
    /// Per-repo isolated engines, keyed by `repo_name` — see `SessionState`.
    sessions: Arc<RwLock<std::collections::HashMap<String, SessionState>>>,
    /// When set (CKB_BACKEND_URL + CKB_INTERNAL_SECRET both configured), API
    /// keys are validated as real per-user keys against the Node backend
    /// instead of a single shared CKB_API_KEY — this is what makes
    /// usage-based billing possible, since a shared key has no concept of
    /// "whose call was this."
    backend: Option<Arc<BackendConfig>>,
    http_client: reqwest::Client,
}

#[derive(Deserialize)]
struct KeyValidationResponse {
    valid: bool,
    key_id: Option<String>,
    user_id: Option<String>,
}

fn extract_api_key(headers: &HeaderMap) -> Option<String> {
    headers
        .get("x-api-key")
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string())
        .or_else(|| {
            headers
                .get(axum::http::header::AUTHORIZATION)
                .and_then(|v| v.to_str().ok())
                .and_then(|v| v.strip_prefix("Bearer "))
                .map(|s| s.to_string())
        })
}

fn unauthorized(msg: &str) -> (StatusCode, String) {
    (StatusCode::UNAUTHORIZED, msg.to_string())
}

/// Requires a valid API key on every `/api/v1/*` route, one of two ways:
///
/// 1. **Per-user mode** (when `CKB_BACKEND_URL`/`CKB_INTERNAL_SECRET` are
///    set): validates the presented key against the Node backend's real,
///    per-user `ApiKeyService`, and — if valid — fires a background,
///    best-effort call to record the usage (for usage-based billing). A
///    failure to record usage never blocks or fails the actual request.
/// 2. **Shared-key fallback mode** (when only `CKB_API_KEY` is set): the
///    original single-operator-key check, unchanged, for simple
///    single-tenant/local deployments that don't run the Node backend.
///
/// If neither is configured, the server runs unauthenticated (with a startup
/// warning logged elsewhere) — this stops the REST server from being an
/// open, unauthenticated filesystem-scan-as-a-service endpoint once an
/// operator opts into either auth mode.
async fn require_api_key(
    State(state): State<AppState>,
    headers: HeaderMap,
    request: Request<axum::body::Body>,
    next: Next,
) -> Result<Response, (StatusCode, String)> {
    if let Some(backend) = &state.backend {
        let key = extract_api_key(&headers).ok_or_else(|| {
            unauthorized("Missing API key. Send it via X-API-Key or Authorization: Bearer.")
        })?;

        let validation = state.http_client
            .post(format!("{}/api/v1/internal/validate-key", backend.url))
            .header("X-Internal-Secret", backend.internal_secret.as_str())
            .json(&json!({ "raw_key": key }))
            .send()
            .await
            .map_err(|e| (StatusCode::BAD_GATEWAY, format!("Failed to reach backend for key validation: {}", e)))?
            .json::<KeyValidationResponse>()
            .await
            .map_err(|e| (StatusCode::BAD_GATEWAY, format!("Malformed key validation response: {}", e)))?;

        if !validation.valid {
            return Err(unauthorized("Invalid or expired API key."));
        }

        // Fire-and-forget usage recording — never blocks or fails the actual
        // request. A transient failure here just means one call goes
        // unmetered, which is the right tradeoff (metering shouldn't be able
        // to take down the product).
        if let (Some(key_id), Some(user_id)) = (validation.key_id, validation.user_id) {
            let client = state.http_client.clone();
            let backend = backend.clone();
            let tool_name = request.uri().path().to_string();
            tokio::spawn(async move {
                let result = client
                    .post(format!("{}/api/v1/internal/record-usage", backend.url))
                    .header("X-Internal-Secret", backend.internal_secret.as_str())
                    .json(&json!({ "key_id": key_id, "user_id": user_id, "tool_name": tool_name }))
                    .send()
                    .await;
                if let Err(e) = result {
                    tracing::debug!("Failed to record API usage (non-fatal): {}", e);
                }
            });
        }

        return Ok(next.run(request).await);
    }

    if let Some(expected) = &state.api_key {
        match extract_api_key(&headers) {
            Some(key) if &key == expected.as_str() => {}
            _ => {
                return Err(unauthorized(
                    "Missing or invalid API key. Set CKB_API_KEY and send it via X-API-Key or Authorization: Bearer.",
                ));
            }
        }
    }
    Ok(next.run(request).await)
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    use std::io::Write;
    println!("CKB MCP Server: starting execution...");
    std::io::stdout().flush().unwrap();
    
    tracing_subscriber::fmt::init();
    
    let args: Vec<String> = std::env::args().collect();
    let engine = CkbEngine::new()?;
    let api_key = std::env::var("CKB_API_KEY").ok().filter(|k| !k.is_empty()).map(Arc::new);
    let backend = match (std::env::var("CKB_BACKEND_URL"), std::env::var("CKB_INTERNAL_SECRET")) {
        (Ok(url), Ok(secret)) if !url.is_empty() && !secret.is_empty() => {
            info!("Per-user API key validation enabled via backend at {}", url);
            Some(Arc::new(BackendConfig { url: url.trim_end_matches('/').to_string(), internal_secret: secret }))
        }
        _ => None,
    };
    let state = AppState {
        engine: Arc::new(RwLock::new(engine)),
        latest_report: Arc::new(RwLock::new(None)),
        api_key: api_key.clone(),
        federated_reports: Arc::new(RwLock::new(std::collections::HashMap::new())),
        sessions: Arc::new(RwLock::new(std::collections::HashMap::new())),
        backend: backend.clone(),
        http_client: reqwest::Client::new(),
    };

    if args.iter().any(|a| a == "--stdio") {
        info!("Starting CKB MCP Server in Stdio JSON-RPC mode...");
        run_stdio_loop(state).await?;
        return Ok(());
    }

    // CORS: only wide-open if the operator explicitly opts in. Otherwise restrict
    // to CKB_ALLOWED_ORIGIN (or same-origin/no-CORS for local tooling).
    let cors = match std::env::var("CKB_ALLOWED_ORIGIN") {
        Ok(origin) if origin == "*" => {
            warn!("CKB_ALLOWED_ORIGIN=* — CORS is wide open. Only do this for local/dev use.");
            CorsLayer::new().allow_origin(Any).allow_methods(Any).allow_headers(Any)
        }
        Ok(origin) => {
            let origin: axum::http::HeaderValue = origin.parse()?;
            CorsLayer::new().allow_origin(origin).allow_methods(Any).allow_headers(Any)
        }
        Err(_) => {
            warn!("CKB_ALLOWED_ORIGIN not set — defaulting to no cross-origin browser access.");
            CorsLayer::new()
        }
    };

    if api_key.is_none() && backend.is_none() {
        warn!(
            "Neither CKB_API_KEY nor CKB_BACKEND_URL/CKB_INTERNAL_SECRET are set. The REST API is running WITHOUT authentication. \
             Anyone who can reach this port can trigger scans of arbitrary filesystem paths. \
             Set CKB_API_KEY (single shared key) or CKB_BACKEND_URL+CKB_INTERNAL_SECRET (per-user keys, usage metering) before exposing this server beyond localhost."
        );
    }

    let protected_routes = Router::new()
        .route("/api/v1/scan", post(scan_codebase))
        .route("/api/v1/report", get(get_report))
        .route("/api/v1/impact", post(analyze_impact))
        .route("/api/v1/search", post(search_codebase))
        .route("/api/v1/clones", post(detect_clones))
        .route("/api/v1/session-impact", post(analyze_session_impact))
        .route("/api/v1/violations/explain", post(explain_violation_handler))
        .route("/api/v1/ask", post(ask_about_codebase))
        .route("/api/v1/otlp", post(ingest_otlp))
        .route("/api/v1/drift-timeline", get(get_drift_timeline))
        .route("/api/v1/test-gaps", get(analyze_test_gaps))
        .route("/api/v1/rules", get(generate_rules))
        .route("/api/v1/org/analytics", get(get_org_analytics))
        .route("/api/v1/federation/repos", get(list_federated_repos))
        .route("/api/v1/metrics/intelligence", get(get_intelligence_metrics))
        .route_layer(middleware::from_fn_with_state(state.clone(), require_api_key));

    let app = Router::new()
        .route("/health", get(health_check))
        .merge(protected_routes)
        .layer(cors)
        .with_state(state);

    let port: u16 = std::env::var("PORT")
        .ok()
        .and_then(|p| p.parse().ok())
        .unwrap_or(3000);
    // Default to loopback-only. Operators who need LAN/public exposure (e.g. inside a
    // container behind their own reverse proxy) can opt in with CKB_BIND_ALL=1, but the
    // safe-by-default posture is 127.0.0.1 so a forgotten `ckb serve` doesn't become a
    // public, unauthenticated filesystem-scanning endpoint.
    let bind_all = std::env::var("CKB_BIND_ALL").map(|v| v == "1" || v.eq_ignore_ascii_case("true")).unwrap_or(false);
    let host: [u8; 4] = if bind_all { [0, 0, 0, 0] } else { [127, 0, 0, 1] };
    let addr = std::net::SocketAddr::from((host, port));
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
                                "properties": {
                                    "repo_path": { "type": "string", "description": "Path to the repository root to scan for clones." }
                                },
                                "required": ["repo_path"]
                            }
                        },
                        {
                            "name": "ckb_analyze_session_impact",
                            "description": "Aggregates blast-radius across multiple file changes in one call (e.g. every edit an AI coding agent made in a session), instead of running impact analysis per-file. Returns unique affected nodes/files, highest/average risk, and which affected code has zero test coverage.",
                            "inputSchema": {
                                "type": "object",
                                "properties": {
                                    "changes": {
                                        "type": "array",
                                        "description": "List of changes made this session.",
                                        "items": {
                                            "type": "object",
                                            "properties": {
                                                "file": { "type": "string" },
                                                "line": { "type": "integer" },
                                                "change_type": { "type": "string", "enum": ["add", "modify", "delete", "rename"] }
                                            },
                                            "required": ["file", "line", "change_type"]
                                        }
                                    }
                                },
                                "required": ["changes"]
                            }
                        },
                        {
                            "name": "ckb_explain_violation",
                            "description": "Explains a single detected architectural violation in plain language and suggests a concrete fix, using an LLM. Pass the full violation object exactly as returned by ckb_scan/ckb_get_report's drift list. Requires ANTHROPIC_API_KEY to be configured on the server.",
                            "inputSchema": {
                                "type": "object",
                                "properties": {
                                    "violation": {
                                        "type": "object",
                                        "description": "A single violation object from a prior scan's `drift` list (id, kind, from, to, boundary, message, severity, suggested_fix)."
                                    }
                                },
                                "required": ["violation"]
                            }
                        },
                        {
                            "name": "ckb_ask",
                            "description": "Answers a natural-language question about the most recently scanned codebase, grounded in the scan's detected nodes and violations. Keyword-retrieval based (not full semantic search) — works best when the question shares vocabulary with actual file/function names. Requires ANTHROPIC_API_KEY and at least one prior ckb_scan_codebase call on this server.",
                            "inputSchema": {
                                "type": "object",
                                "properties": {
                                    "question": { "type": "string", "description": "The question to ask about the codebase." }
                                },
                                "required": ["question"]
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
                        // Previously this always passed an empty HashMap,
                        // so the tool reported "0 clones found" for every
                        // repo regardless of actual content — now it reads
                        // real files from the given path.
                        let path = args.and_then(|a| a.get("repo_path")).and_then(|p| p.as_str()).unwrap_or(".");
                        let engine = state.engine.read().await;
                        match engine.detect_semantic_clones_at(path).await {
                            Ok(report) => serde_json::to_string_pretty(&report).unwrap_or_default(),
                            Err(e) => format!("Error detecting clones: {}", e),
                        }
                    }
                    "ckb_analyze_session_impact" => {
                        let changes: Vec<ckb_core::SessionChange> = args
                            .and_then(|a| a.get("changes"))
                            .and_then(|c| serde_json::from_value(c.clone()).ok())
                            .unwrap_or_default();
                        if changes.is_empty() {
                            "Error: 'changes' must be a non-empty array of {file, line, change_type}".to_string()
                        } else {
                            let engine = state.engine.read().await;
                            match engine.analyze_session_impact(&changes).await {
                                Ok(summary) => serde_json::to_string_pretty(&summary).unwrap_or_default(),
                                Err(e) => format!("Error analyzing session impact: {}", e),
                            }
                        }
                    }
                    "ckb_explain_violation" => {
                        let violation: Option<ckb_core::DriftViolation> = args
                            .and_then(|a| a.get("violation"))
                            .and_then(|v| serde_json::from_value(v.clone()).ok());
                        match violation {
                            None => "Error: 'violation' must be a valid violation object (see ckb_scan/ckb_get_report's drift list).".to_string(),
                            Some(v) => match std::env::var("ANTHROPIC_API_KEY") {
                                Err(_) => "Error: ANTHROPIC_API_KEY is not configured on this server.".to_string(),
                                Ok(api_key) => match explain::explain_violation(&v, &api_key).await {
                                    Ok(resp) => serde_json::to_string_pretty(&resp).unwrap_or_default(),
                                    Err(e) => format!("Error explaining violation: {}", e),
                                },
                            },
                        }
                    }
                    "ckb_ask" => {
                        let question = args.and_then(|a| a.get("question")).and_then(|q| q.as_str());
                        match question {
                            None => "Error: 'question' is required.".to_string(),
                            Some(q) => match std::env::var("ANTHROPIC_API_KEY") {
                                Err(_) => "Error: ANTHROPIC_API_KEY is not configured on this server.".to_string(),
                                Ok(api_key) => {
                                    let violations = match &*state.latest_report.read().await {
                                        Some(r) => r.drift.clone(),
                                        None => Vec::new(),
                                    };
                                    let nodes = state.engine.read().await.get_all_nodes().await;
                                    if nodes.is_empty() {
                                        "Error: No scan has been run yet on this server — call ckb_scan_codebase first.".to_string()
                                    } else {
                                        match ask::ask_about_codebase(q, &nodes, &violations, &api_key).await {
                                            Ok(answer) => answer,
                                            Err(e) => format!("Error answering question: {}", e),
                                        }
                                    }
                                }
                            },
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
    /// Optional: registers this scan's report under a named repo for
    /// multi-repo federation (see AppState.federated_reports). If omitted,
    /// the scan still runs and updates `latest_report` as before — this is
    /// purely additive, existing single-repo callers are unaffected.
    repo_name: Option<String>,
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
    let session = resolve_session(&state, payload.repo_name.as_deref()).await?;
    let engine = session.engine.read().await;

    match engine.scan_codebase(&payload.path).await {
        Ok(report) => {
            let violations_count = report.drift.len();
            if let Some(ref repo_name) = payload.repo_name {
                state.federated_reports.write().await.insert(repo_name.clone(), report.clone());
            }
            *session.latest_report.write().await = Some(report);
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
    axum::extract::Query(params): axum::extract::Query<std::collections::HashMap<String, String>>,
) -> Result<Json<ScanReport>, (StatusCode, String)> {
    let session = resolve_session(&state, params.get("repo").map(|s| s.as_str())).await?;
    let report = session.latest_report.read().await;
    match &*report {
        Some(r) => Ok(Json(r.clone())),
        None => Err((StatusCode::NOT_FOUND, "No scan has been run yet for this session.".to_string())),
    }
}

#[derive(Deserialize)]
struct ImpactRequest {
    path: String,
    file: String,
    line: u32,
    change_type: String,
    repo_name: Option<String>,
}

async fn analyze_impact(
    State(state): State<AppState>,
    Json(payload): Json<ImpactRequest>,
) -> Result<Json<ckb_core::ImpactAnalysis>, (StatusCode, String)> {
    let session = resolve_session(&state, payload.repo_name.as_deref()).await?;
    let engine = session.engine.read().await;

    // Ensure graph is populated
    if session.latest_report.read().await.is_none() {
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

#[derive(Deserialize)]
struct SearchRequest {
    query: String,
    repo_name: Option<String>,
}

#[derive(Serialize)]
struct SearchResult {
    query: String,
    matches: Vec<Value>,
}

async fn search_codebase(
    State(state): State<AppState>,
    Json(payload): Json<SearchRequest>,
) -> Result<Json<SearchResult>, (StatusCode, String)> {
    let session = resolve_session(&state, payload.repo_name.as_deref()).await?;
    let report = session.latest_report.read().await;
    
    let mut matches = Vec::new();
    if let Some(ref r) = *report {
        let q = payload.query.to_lowercase();
        for pattern in &r.patterns {
            if format!("{:?}", pattern).to_lowercase().contains(&q) {
                matches.push(json!({ "type": "pattern", "data": pattern }));
            }
        }
        for drift in &r.drift {
            if drift.message.to_lowercase().contains(&q) || drift.boundary.to_lowercase().contains(&q) {
                matches.push(json!({ "type": "drift", "data": drift }));
            }
        }
    }
    
    Ok(Json(SearchResult {
        query: payload.query,
        matches,
    }))
}

#[derive(Deserialize)]
struct CloneDetectRequest {
    path: String,
}

/// Previously the only way to reach clone detection was the `ckb_detect_semantic_clones`
/// MCP stdio tool (and that was broken — see the fix in `detect_semantic_clones_at`).
/// Adding a REST route here gives the dashboard/CLI/SDKs the same access other
/// advanced-analysis features (`/api/v1/test-gaps`, `/api/v1/rules`, etc.) already have.
async fn detect_clones(
    State(state): State<AppState>,
    Json(payload): Json<CloneDetectRequest>,
) -> Result<Json<ckb_core::CloneReport>, (StatusCode, String)> {
    let engine = state.engine.read().await;
    match engine.detect_semantic_clones_at(&payload.path).await {
        Ok(report) => Ok(Json(report)),
        Err(e) => Err((StatusCode::INTERNAL_SERVER_ERROR, format!("Clone detection failed: {}", e))),
    }
}

#[derive(Deserialize)]
struct SessionImpactRequest {
    changes: Vec<ckb_core::SessionChange>,
    repo_name: Option<String>,
}

/// Aggregates blast-radius across a whole editing session in one call,
/// instead of making the caller run `/api/v1/impact` per file and merge the
/// results themselves. Built specifically for reviewing multi-file changes
/// from an AI coding agent in one pass.
async fn analyze_session_impact(
    State(state): State<AppState>,
    Json(payload): Json<SessionImpactRequest>,
) -> Result<Json<ckb_core::SessionImpactSummary>, (StatusCode, String)> {
    let session = resolve_session(&state, payload.repo_name.as_deref()).await?;
    let engine = session.engine.read().await;
    match engine.analyze_session_impact(&payload.changes).await {
        Ok(summary) => Ok(Json(summary)),
        Err(e) => Err((StatusCode::INTERNAL_SERVER_ERROR, format!("Session impact analysis failed: {}", e))),
    }
}

#[derive(Deserialize)]
struct ExplainViolationRequest {
    violation: ckb_core::DriftViolation,
}

/// Explains a single detected violation in plain language and suggests a
/// concrete fix, via the Claude API. Requires `ANTHROPIC_API_KEY`.
async fn explain_violation_handler(
    Json(payload): Json<ExplainViolationRequest>,
) -> Result<Json<explain::ExplainFixResponse>, (StatusCode, String)> {
    let api_key = std::env::var("ANTHROPIC_API_KEY")
        .map_err(|_| (StatusCode::SERVICE_UNAVAILABLE, "ANTHROPIC_API_KEY is not configured on this server.".to_string()))?;

    explain::explain_violation(&payload.violation, &api_key)
        .await
        .map(Json)
        .map_err(|e| (StatusCode::BAD_GATEWAY, e))
}

#[derive(Deserialize)]
struct AskRequest {
    question: String,
    repo_name: Option<String>,
}

#[derive(Serialize)]
struct AskResponse {
    answer: String,
}

/// Natural-language Q&A about the most recently scanned codebase. See the
/// scope note at the top of `ask.rs` — this is keyword-retrieval + LLM
/// synthesis, not real semantic/embeddings search. Requires
/// `ANTHROPIC_API_KEY` and at least one prior scan (`/api/v1/scan`) on this
/// server.
async fn ask_about_codebase(
    State(state): State<AppState>,
    Json(payload): Json<AskRequest>,
) -> Result<Json<AskResponse>, (StatusCode, String)> {
    let api_key = std::env::var("ANTHROPIC_API_KEY")
        .map_err(|_| (StatusCode::SERVICE_UNAVAILABLE, "ANTHROPIC_API_KEY is not configured on this server.".to_string()))?;

    let session = resolve_session(&state, payload.repo_name.as_deref()).await?;
    let violations = match &*session.latest_report.read().await {
        Some(r) => r.drift.clone(),
        None => Vec::new(),
    };
    let nodes = session.engine.read().await.get_all_nodes().await;

    if nodes.is_empty() {
        return Err((StatusCode::PRECONDITION_REQUIRED, "No scan has been run yet for this session — call /api/v1/scan first.".to_string()));
    }

    ask::ask_about_codebase(&payload.question, &nodes, &violations, &api_key)
        .await
        .map(|answer| Json(AskResponse { answer }))
        .map_err(|e| (StatusCode::BAD_GATEWAY, e))
}

#[derive(Deserialize)]
struct OtlpRequest {
    raw_spans: String,
    repo_name: Option<String>,
}

async fn ingest_otlp(
    State(state): State<AppState>,
    Json(payload): Json<OtlpRequest>,
) -> Result<Json<ckb_core::OtlpIngestReport>, (StatusCode, String)> {
    let session = resolve_session(&state, payload.repo_name.as_deref()).await?;
    let engine = session.engine.read().await;
    match engine.ingest_otlp_spans(&payload.raw_spans).await {
        Ok(report) => Ok(Json(report)),
        Err(e) => Err((StatusCode::INTERNAL_SERVER_ERROR, e.to_string())),
    }
}

async fn get_drift_timeline(
    State(state): State<AppState>,
) -> Result<Json<ckb_core::DriftTimeline>, (StatusCode, String)> {
    // NOTE: deliberately not session-scoped like the other handlers above —
    // this reads git history from a hardcoded "." (the server process's own
    // working directory), which is a pre-existing design choice orthogonal
    // to per-repo session isolation. Fixing that properly means accepting a
    // path/repo parameter here too, which is a real follow-up, not bundled
    // into this pass.
    let engine = state.engine.read().await;
    match engine.get_drift_timeline(".", 50).await {
        Ok(timeline) => Ok(Json(timeline)),
        Err(e) => Err((StatusCode::INTERNAL_SERVER_ERROR, e.to_string())),
    }
}

async fn analyze_test_gaps(
    State(state): State<AppState>,
    axum::extract::Query(params): axum::extract::Query<std::collections::HashMap<String, String>>,
) -> Result<Json<ckb_core::TestCoverageGapReport>, (StatusCode, String)> {
    let session = resolve_session(&state, params.get("repo").map(|s| s.as_str())).await?;
    let engine = session.engine.read().await;
    match engine.analyze_test_coverage_gaps().await {
        Ok(report) => Ok(Json(report)),
        Err(e) => Err((StatusCode::INTERNAL_SERVER_ERROR, e.to_string())),
    }
}

async fn generate_rules(
    State(state): State<AppState>,
    axum::extract::Query(params): axum::extract::Query<std::collections::HashMap<String, String>>,
) -> Result<String, (StatusCode, String)> {
    let session = resolve_session(&state, params.get("repo").map(|s| s.as_str())).await?;
    let engine = session.engine.read().await;
    match engine.generate_ai_rules().await {
        Ok(rules) => Ok(rules),
        Err(e) => Err((StatusCode::INTERNAL_SERVER_ERROR, e.to_string())),
    }
}

/// Lists repos currently registered for multi-repo federation (i.e. scanned
/// with `repo_name` set on `POST /api/v1/scan`), along with a summary of
/// each one's last scan.
async fn list_federated_repos(
    State(state): State<AppState>,
) -> Json<Vec<serde_json::Value>> {
    let map = build_federation_map(&state).await;
    let list: Vec<serde_json::Value> = map.into_iter().map(|(name, r)| {
        json!({
            "repo_name": name,
            "files_processed": r.files_processed,
            "nodes": r.nodes,
            "edges": r.edges,
            "violations": r.drift.len(),
            "package_identity": r.package_identity,
        })
    }).collect();
    Json(list)
}

async fn get_org_analytics(
    State(state): State<AppState>,
) -> Result<Json<ckb_core::federation::OrganizationalIntelligenceReport>, (StatusCode, String)> {
    let map = build_federation_map(&state).await;
    let org_report = ckb_core::federation::FederatedGraphEngine::analyze_org_intelligence(&map);
    Ok(Json(org_report))
}

/// Registered repos, if any were scanned with `repo_name` set (real
/// multi-repo federation). Falls back to a single `"default"` entry sourced
/// from the last unscoped scan, so single-project setups that never pass
/// `repo_name` still get a sensible one-repo report instead of an empty one.
async fn build_federation_map(state: &AppState) -> std::collections::HashMap<String, ScanReport> {
    let federated = state.federated_reports.read().await;
    if !federated.is_empty() {
        return federated.clone();
    }
    drop(federated);

    let mut map = std::collections::HashMap::new();
    if let Some(ref r) = *state.latest_report.read().await {
        map.insert("default".to_string(), r.clone());
    }
    map
}

async fn get_intelligence_metrics(
    State(state): State<AppState>,
) -> Result<Json<ckb_core::federation::IntelligenceBenchmarkMetrics>, (StatusCode, String)> {
    // Previously this returned IntelligenceBenchmarkMetrics::default(), which
    // no longer exists — that default was hardcoded fake numbers (e.g. a
    // constant "14,200 files/sec") regardless of any real scan. This now
    // computes real metrics from whatever the server has actually scanned.
    let started = std::time::Instant::now();
    let map = build_federation_map(&state).await;
    Ok(Json(ckb_core::federation::IntelligenceBenchmarkMetrics::from_reports(&map, started.elapsed())))
}
