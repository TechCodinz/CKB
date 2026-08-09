use axum::{
    extract::{DefaultBodyLimit, Query, State},
    http::{HeaderMap, StatusCode},
    middleware::{self, Next},
    response::Response,
    routing::{get, post},
    Json, Router,
};
use base64::{engine::general_purpose::STANDARD as BASE64, Engine as _};
use ckb_core::{
    ArchitectureAnalyzer, ArchitectureMemoryEngine, CausalArchitectureEngine, ChangeType,
    DependencyGraph, FileAnalysis, GitDriftAnalyzer, LanguageParser, Node, NodeId,
    OtlpReceiver, RuntimeMetrics, ScanReport, TestCoverageAnalyzer,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    collections::{BTreeSet, HashMap, HashSet},
    io::{Cursor, Read, Write},
    path::{Path, PathBuf},
    sync::Arc,
};
use tokio::sync::RwLock;
use tower_http::cors::{Any, CorsLayer};
use tracing::{info, warn};
use zip::ZipArchive;

const MAX_ARCHIVE_BYTES: usize = 60 * 1024 * 1024;
const MAX_EXTRACTED_BYTES: u64 = 300 * 1024 * 1024;
const MAX_ARCHIVE_FILES: usize = 30_000;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RuntimeEdgeObservation {
    source: String,
    target: String,
    trace_id: String,
    invocation_count: u64,
    error_count: u64,
    total_latency_ms: f64,
    last_seen_unix_nano: u64,
}
impl RuntimeEdgeObservation {
    fn error_rate(&self) -> f64 {
        if self.invocation_count == 0 { 0.0 } else { self.error_count as f64 / self.invocation_count as f64 }
    }
    fn avg_latency_ms(&self) -> f64 {
        if self.invocation_count == 0 { 0.0 } else { self.total_latency_ms / self.invocation_count as f64 }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct PersistedSession {
    graph: Option<DependencyGraph>,
    report: Option<ScanReport>,
    repo_path: Option<String>,
    repo_url: Option<String>,
    runtime_nodes: HashMap<NodeId, RuntimeMetrics>,
    runtime_edges: HashMap<String, RuntimeEdgeObservation>,
    saved_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ArchitectureSnapshot {
    id: String,
    project_key: String,
    created_at: String,
    report: ScanReport,
    graph: DependencyGraph,
}

#[derive(Clone)]
struct Session {
    graph: Arc<RwLock<Option<DependencyGraph>>>,
    report: Arc<RwLock<Option<ScanReport>>>,
    repo_path: Arc<RwLock<Option<String>>>,
    repo_url: Arc<RwLock<Option<String>>>,
    runtime_nodes: Arc<RwLock<HashMap<NodeId, RuntimeMetrics>>>,
    runtime_edges: Arc<RwLock<HashMap<String, RuntimeEdgeObservation>>>,
    project_key: String,
}
impl Session {
    fn empty(project_key: String) -> Self {
        Self {
            graph: Arc::new(RwLock::new(None)),
            report: Arc::new(RwLock::new(None)),
            repo_path: Arc::new(RwLock::new(None)),
            repo_url: Arc::new(RwLock::new(None)),
            runtime_nodes: Arc::new(RwLock::new(HashMap::new())),
            runtime_edges: Arc::new(RwLock::new(HashMap::new())),
            project_key,
        }
    }
}

#[derive(Clone)]
struct AppState {
    sessions: Arc<RwLock<HashMap<String, Session>>>,
    api_key: Option<Arc<String>>,
    data_dir: Arc<PathBuf>,
    http: reqwest::Client,
}

#[derive(Debug, Deserialize)]
struct ScanRequest {
    path: String,
    repo_name: Option<String>,
    project_id: Option<String>,
}
#[derive(Debug, Deserialize)]
struct GitHubScanRequest {
    github_url: String,
    github_token: Option<String>,
    project_id: Option<String>,
    repo_name: Option<String>,
}
#[derive(Debug, Deserialize)]
struct ZipScanRequest {
    file_data: String,
    file_name: Option<String>,
    project_id: Option<String>,
    repo_name: Option<String>,
}
#[derive(Debug, Deserialize)]
struct ImpactRequest {
    path: Option<String>,
    file: String,
    #[serde(default = "default_line")]
    line: u32,
    change_type: Option<String>,
    repo_name: Option<String>,
    project_id: Option<String>,
}
#[derive(Debug, Deserialize)]
struct OtlpRequest {
    raw_spans: Option<String>,
    otlp_json: Option<String>,
    payload: Option<Value>,
    repo_name: Option<String>,
    project_id: Option<String>,
}
#[derive(Debug, Deserialize)]
struct DiffRequest {
    from_snapshot: String,
    to_snapshot: String,
    project_id: Option<String>,
    repo_name: Option<String>,
}
#[derive(Debug, Deserialize)]
struct CausalPathRequest {
    source: String,
    target: String,
    max_depth: Option<usize>,
    project_id: Option<String>,
    repo_name: Option<String>,
}
#[derive(Debug, Deserialize)]
struct FailureConeRequest {
    root: String,
    max_depth: Option<usize>,
    project_id: Option<String>,
    repo_name: Option<String>,
}
#[derive(Debug, Deserialize)]
struct MemoryQueryRequest {
    query: String,
    depth: Option<usize>,
    limit: Option<usize>,
    project_id: Option<String>,
    repo_name: Option<String>,
}

fn default_line() -> u32 { 1 }
fn internal<E: std::fmt::Display>(e: E) -> (StatusCode, String) {
    (StatusCode::INTERNAL_SERVER_ERROR, e.to_string())
}
fn bad<E: std::fmt::Display>(e: E) -> (StatusCode, String) {
    (StatusCode::BAD_REQUEST, e.to_string())
}
fn key(repo: Option<&str>, project: Option<&str>) -> String {
    project.filter(|s| !s.is_empty())
        .or_else(|| repo.filter(|s| !s.is_empty()))
        .unwrap_or("default")
        .to_string()
}
fn safe_key(k: &str) -> String {
    k.chars().map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '_' { c } else { '_' }).collect()
}
fn session_file(state: &AppState, project_key: &str) -> PathBuf {
    state.data_dir.join("sessions").join(format!("{}.bin", safe_key(project_key)))
}
fn snapshot_dir(state: &AppState, project_key: &str) -> PathBuf {
    state.data_dir.join("snapshots").join(safe_key(project_key))
}

async fn load_session(state: &AppState, project_key: &str) -> Session {
    if let Some(s) = state.sessions.read().await.get(project_key) { return s.clone(); }
    let s = Session::empty(project_key.to_string());
    if let Ok(bytes) = tokio::fs::read(session_file(state, project_key)).await {
        if let Ok(p) = bincode::deserialize::<PersistedSession>(&bytes) {
            *s.graph.write().await = p.graph;
            *s.report.write().await = p.report;
            *s.repo_path.write().await = p.repo_path;
            *s.repo_url.write().await = p.repo_url;
            *s.runtime_nodes.write().await = p.runtime_nodes;
            *s.runtime_edges.write().await = p.runtime_edges;
        }
    }
    state.sessions.write().await.insert(project_key.to_string(), s.clone());
    s
}

async fn persist_session(state: &AppState, s: &Session) -> anyhow::Result<()> {
    let persisted = PersistedSession {
        graph: s.graph.read().await.clone(),
        report: s.report.read().await.clone(),
        repo_path: s.repo_path.read().await.clone(),
        repo_url: s.repo_url.read().await.clone(),
        runtime_nodes: s.runtime_nodes.read().await.clone(),
        runtime_edges: s.runtime_edges.read().await.clone(),
        saved_at: chrono::Utc::now().to_rfc3339(),
    };
    let path = session_file(state, &s.project_key);
    if let Some(parent) = path.parent() { tokio::fs::create_dir_all(parent).await?; }
    let temp = path.with_extension("bin.tmp");
    tokio::fs::write(&temp, bincode::serialize(&persisted)?).await?;
    tokio::fs::rename(temp, path).await?;
    Ok(())
}

async fn persist_snapshot(state: &AppState, s: &Session) -> anyhow::Result<Option<String>> {
    let graph = s.graph.read().await.clone();
    let report = s.report.read().await.clone();
    let (Some(graph), Some(report)) = (graph, report) else { return Ok(None); };
    let id = report.snapshot_id.clone();
    let snap = ArchitectureSnapshot {
        id: id.clone(),
        project_key: s.project_key.clone(),
        created_at: chrono::Utc::now().to_rfc3339(),
        report,
        graph,
    };
    let dir = snapshot_dir(state, &s.project_key);
    tokio::fs::create_dir_all(&dir).await?;
    tokio::fs::write(dir.join(format!("{}.bin", safe_key(&id))), bincode::serialize(&snap)?).await?;
    Ok(Some(id))
}

async fn read_snapshot(state: &AppState, project_key: &str, id: &str) -> anyhow::Result<ArchitectureSnapshot> {
    let path = snapshot_dir(state, project_key).join(format!("{}.bin", safe_key(id)));
    let bytes = tokio::fs::read(path).await?;
    Ok(bincode::deserialize(&bytes)?)
}

fn extract_key(headers: &HeaderMap) -> Option<String> {
    headers.get("x-api-key").and_then(|v| v.to_str().ok()).map(str::to_string)
        .or_else(|| headers.get(axum::http::header::AUTHORIZATION)
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.strip_prefix("Bearer "))
            .map(str::to_string))
}
async fn auth(
    State(state): State<AppState>,
    headers: HeaderMap,
    request: axum::http::Request<axum::body::Body>,
    next: Next,
) -> Result<Response, (StatusCode, String)> {
    if let Some(expected) = &state.api_key {
        if extract_key(&headers).as_deref() != Some(expected.as_str()) {
            return Err((StatusCode::UNAUTHORIZED, "Missing or invalid CKB API key".into()));
        }
    }
    Ok(next.run(request).await)
}

fn supported(path: &Path) -> bool {
    matches!(path.extension().and_then(|v| v.to_str()).unwrap_or(""), "ts"|"tsx"|"js"|"jsx"|"mjs"|"py"|"go"|"rs"|"java")
}
fn discover(root: &str) -> anyhow::Result<Vec<PathBuf>> {
    let mut out = Vec::new();
    let mut stack = vec![PathBuf::from(root)];
    while let Some(dir) = stack.pop() {
        for entry in std::fs::read_dir(&dir)? {
            let entry = entry?;
            let p = entry.path();
            let name = p.file_name().and_then(|v| v.to_str()).unwrap_or("");
            if p.is_dir() {
                if !matches!(name, ".git"|"node_modules"|"target"|"dist"|"build"|".next"|"vendor"|"coverage"|".turbo"|".yarn") {
                    stack.push(p);
                }
            } else if supported(&p) {
                out.push(p);
            }
        }
    }
    Ok(out)
}

fn package_identity(root: &str) -> Option<String> {
    let r = Path::new(root);
    if let Ok(s) = std::fs::read_to_string(r.join("package.json")) {
        if let Ok(v) = serde_json::from_str::<Value>(&s) {
            if let Some(n) = v.get("name").and_then(Value::as_str) { return Some(n.into()); }
        }
    }
    if let Ok(s) = std::fs::read_to_string(r.join("go.mod")) {
        for l in s.lines() { if let Some(v) = l.trim().strip_prefix("module ") { return Some(v.trim().into()); } }
    }
    for file in ["Cargo.toml", "pyproject.toml"] {
        if let Ok(s) = std::fs::read_to_string(r.join(file)) {
            for l in s.lines() {
                let l = l.trim();
                if let Some(v) = l.strip_prefix("name").and_then(|v| v.trim_start().strip_prefix('=')) {
                    let n = v.trim().trim_matches('"');
                    if !n.is_empty() { return Some(n.into()); }
                }
            }
        }
    }
    None
}
fn external_dependencies(analyses: &[FileAnalysis]) -> Vec<String> {
    let mut deps = BTreeSet::new();
    for a in analyses {
        for i in &a.imports {
            if i.source.starts_with('.') || i.source.starts_with('/') || i.source.is_empty() { continue; }
            let name = if let Some(s) = i.source.strip_prefix('@') {
                let mut p = s.split('/');
                match (p.next(), p.next()) {
                    (Some(a), Some(b)) => format!("@{}/{}", a, b),
                    _ => i.source.clone(),
                }
            } else {
                i.source.split('/').next().unwrap_or(&i.source).to_string()
            };
            deps.insert(name);
        }
    }
    deps.into_iter().collect()
}

async fn build_graph(path: &str) -> anyhow::Result<(DependencyGraph, ScanReport)> {
    let started = std::time::Instant::now();
    let parser = LanguageParser::new();
    let files = discover(path)?;
    let mut analyses = Vec::new();
    for p in files {
        let s = p.to_string_lossy().to_string();
        if let Ok(a) = parser.parse_file(&s).await { analyses.push(a); }
    }
    if analyses.is_empty() { anyhow::bail!("No supported source files could be parsed"); }
    let mut graph = DependencyGraph::new();
    for a in &analyses { graph.add_file(a)?; }
    graph.build_call_graph()?;
    graph.build_type_graph()?;
    let analyzer = ArchitectureAnalyzer::new();
    let patterns = analyzer.detect_patterns(&graph)?;
    let drift = analyzer.detect_drift(&graph, &patterns)?;
    let report = ScanReport {
        files_processed: analyses.len(),
        nodes: graph.node_count(),
        edges: graph.edge_count(),
        patterns,
        drift,
        snapshot_id: uuid::Uuid::new_v4().to_string(),
        duration_ms: started.elapsed().as_secs_f64() * 1000.0,
        package_identity: package_identity(path),
        external_dependencies: external_dependencies(&analyses),
    };
    Ok((graph, report))
}

fn parse_github_url(raw: &str) -> Option<(String, String)> {
    let clean = raw.trim().trim_end_matches('/').trim_end_matches(".git");
    let marker = "github.com/";
    let idx = clean.find(marker)? + marker.len();
    let mut parts = clean[idx..].split('/');
    let owner = parts.next()?.trim();
    let repo = parts.next()?.trim();
    if owner.is_empty() || repo.is_empty() { return None; }
    Some((owner.to_string(), repo.to_string()))
}

fn extract_zip_safely(bytes: &[u8], target: &Path) -> anyhow::Result<PathBuf> {
    if bytes.len() > MAX_ARCHIVE_BYTES { anyhow::bail!("Archive exceeds {} MB limit", MAX_ARCHIVE_BYTES / 1024 / 1024); }
    std::fs::create_dir_all(target)?;
    let mut zip = ZipArchive::new(Cursor::new(bytes))?;
    if zip.len() > MAX_ARCHIVE_FILES { anyhow::bail!("Archive contains too many files"); }
    let mut total = 0u64;
    for i in 0..zip.len() {
        let mut entry = zip.by_index(i)?;
        total = total.saturating_add(entry.size());
        if total > MAX_EXTRACTED_BYTES { anyhow::bail!("Expanded archive exceeds safety limit"); }
        let Some(rel) = entry.enclosed_name().map(Path::to_path_buf) else { continue; };
        let out = target.join(rel);
        if entry.is_dir() {
            std::fs::create_dir_all(&out)?;
            continue;
        }
        if let Some(parent) = out.parent() { std::fs::create_dir_all(parent)?; }
        let mut file = std::fs::File::create(&out)?;
        std::io::copy(&mut entry, &mut file)?;
        file.flush()?;
    }
    let mut entries = std::fs::read_dir(target)?.filter_map(Result::ok).collect::<Vec<_>>();
    if entries.len() == 1 && entries[0].path().is_dir() { Ok(entries.remove(0).path()) } else { Ok(target.to_path_buf()) }
}

async fn save_scan(
    state: &AppState,
    project_key: String,
    graph: DependencyGraph,
    report: ScanReport,
    repo_path: Option<String>,
    repo_url: Option<String>,
) -> Result<Json<Value>, (StatusCode, String)> {
    let session = load_session(state, &project_key).await;
    *session.graph.write().await = Some(graph);
    *session.report.write().await = Some(report.clone());
    *session.repo_path.write().await = repo_path;
    *session.repo_url.write().await = repo_url;
    session.runtime_nodes.write().await.clear();
    session.runtime_edges.write().await.clear();
    persist_session(state, &session).await.map_err(internal)?;
    let snapshot_id = persist_snapshot(state, &session).await.map_err(internal)?;
    Ok(Json(json!({
        "status":"success",
        "projectKey":project_key,
        "filesProcessed":report.files_processed,
        "nodes":report.nodes,
        "edges":report.edges,
        "violationsFound":report.drift.len(),
        "snapshotId":snapshot_id,
        "engine":"tree-sitter-rust-reality-v4",
        "synthetic":false
    })))
}

async fn health(State(state): State<AppState>) -> Json<Value> {
    Json(json!({
        "status":"healthy",
        "service":"ckb-reality-server-v4",
        "realityApi":"v4",
        "remoteGitHubScan":true,
        "zipScan":true,
        "graphPersistence":"durable-bincode-snapshots",
        "dataDir":state.data_dir.to_string_lossy(),
        "evidencePolicy":"static-runtime-predicted-separated"
    }))
}

async fn scan(State(state): State<AppState>, Json(req): Json<ScanRequest>) -> Result<Json<Value>, (StatusCode, String)> {
    let project_key = key(req.repo_name.as_deref(), req.project_id.as_deref());
    let (graph, report) = build_graph(&req.path).await.map_err(internal)?;
    save_scan(&state, project_key, graph, report, Some(req.path), None).await
}

async fn scan_github(State(state): State<AppState>, Json(req): Json<GitHubScanRequest>) -> Result<Json<Value>, (StatusCode, String)> {
    let (owner, repo) = parse_github_url(&req.github_url).ok_or_else(|| bad("Invalid GitHub URL"))?;
    let url = format!("https://api.github.com/repos/{}/{}/zipball/HEAD", owner, repo);
    let mut request = state.http.get(url)
        .header("Accept", "application/vnd.github+json")
        .header("User-Agent", "CKB-Software-Reality-v4")
        .header("X-GitHub-Api-Version", "2022-11-28");
    if let Some(token) = req.github_token.as_deref().filter(|v| !v.trim().is_empty()) {
        request = request.bearer_auth(token.trim());
    }
    let response = request.send().await.map_err(internal)?;
    if response.status() == reqwest::StatusCode::NOT_FOUND {
        return Err((StatusCode::NOT_FOUND, "Repository not found or token lacks access".into()));
    }
    if response.status() == reqwest::StatusCode::UNAUTHORIZED || response.status() == reqwest::StatusCode::FORBIDDEN {
        return Err((StatusCode::FORBIDDEN, "GitHub repository access denied".into()));
    }
    if !response.status().is_success() {
        return Err((StatusCode::BAD_GATEWAY, format!("GitHub returned {}", response.status())));
    }
    let bytes = response.bytes().await.map_err(internal)?;
    let temp = std::env::temp_dir().join(format!("ckb-reality-{}", uuid::Uuid::new_v4()));
    let repo_root = extract_zip_safely(&bytes, &temp).map_err(bad)?;
    let built = build_graph(&repo_root.to_string_lossy()).await;
    let _ = std::fs::remove_dir_all(&temp);
    let (graph, report) = built.map_err(internal)?;
    let project_key = key(req.repo_name.as_deref(), req.project_id.as_deref());
    save_scan(&state, project_key, graph, report, None, Some(format!("https://github.com/{}/{}", owner, repo))).await
}

async fn scan_zip(State(state): State<AppState>, Json(req): Json<ZipScanRequest>) -> Result<Json<Value>, (StatusCode, String)> {
    let payload = req.file_data.rsplit(',').next().unwrap_or(&req.file_data);
    let bytes = BASE64.decode(payload.as_bytes()).map_err(bad)?;
    let temp = std::env::temp_dir().join(format!("ckb-zip-{}", uuid::Uuid::new_v4()));
    let repo_root = extract_zip_safely(&bytes, &temp).map_err(bad)?;
    let built = build_graph(&repo_root.to_string_lossy()).await;
    let _ = std::fs::remove_dir_all(&temp);
    let (graph, report) = built.map_err(internal)?;
    let project_key = key(req.repo_name.as_deref().or(req.file_name.as_deref()), req.project_id.as_deref());
    save_scan(&state, project_key, graph, report, None, req.file_name).await
}

async fn report(State(state): State<AppState>, Query(q): Query<HashMap<String,String>>) -> Result<Json<ScanReport>, (StatusCode,String)> {
    let session = load_session(&state, &key(q.get("repo").map(String::as_str), q.get("project_id").map(String::as_str))).await;
    let value = session.report.read().await.clone();
    value.map(Json).ok_or((StatusCode::NOT_FOUND, "No scan has been run for this project".into()))
}

fn change_type(value: Option<&str>) -> ChangeType {
    match value.unwrap_or("modify").to_ascii_lowercase().as_str() {
        "add" => ChangeType::Add,
        "delete" => ChangeType::Delete,
        "rename" => ChangeType::Rename,
        _ => ChangeType::Modify,
    }
}
fn kind<T: std::fmt::Debug>(value: T) -> String { format!("{:?}", value).to_ascii_lowercase() }

async fn impact(State(state): State<AppState>, Json(req): Json<ImpactRequest>) -> Result<Json<Value>, (StatusCode,String)> {
    let session = load_session(&state, &key(req.repo_name.as_deref(), req.project_id.as_deref())).await;
    if session.graph.read().await.is_none() {
        let path = req.path.as_deref().ok_or((StatusCode::PRECONDITION_REQUIRED, "Scan first or provide a local path".into()))?;
        let (graph, report) = build_graph(path).await.map_err(internal)?;
        *session.graph.write().await = Some(graph);
        *session.report.write().await = Some(report);
        *session.repo_path.write().await = Some(path.into());
        persist_session(&state, &session).await.map_err(internal)?;
    }
    let graph = session.graph.read().await;
    let graph = graph.as_ref().ok_or((StatusCode::PRECONDITION_REQUIRED, "No graph".into()))?;
    let affected = graph.find_affected_nodes(&req.file, req.line).map_err(internal)?;
    let result = graph.calculate_impact(&affected, change_type(req.change_type.as_deref())).map_err(internal)?;
    Ok(Json(json!({
        "kind":"predicted",
        "confidencePolicy":"derived-per-path",
        "assumptions":["Current persisted graph is the baseline"],
        "evidence":[{"source":"ast-graph","ref":format!("{}:{}", req.file, req.line)}],
        "result":result,
        "synthetic":false
    })))
}

fn runtime_for(runtime: &HashMap<NodeId, RuntimeMetrics>, node: &Node) -> Option<RuntimeMetrics> {
    if let Some(m) = runtime.get(&node.id) { return Some(m.clone()); }
    let p = node.path.to_string_lossy().replace('\\', "/");
    runtime.iter().find_map(|(id, m)| {
        let raw = id.0.replace('\\', "/");
        if raw == node.name || raw.ends_with(&format!("::{}", node.name)) || (raw.starts_with(&format!("{}::", p)) && raw.ends_with(&node.name)) {
            Some(m.clone())
        } else { None }
    })
}

fn graph_json(graph: &DependencyGraph, runtime_nodes: &HashMap<NodeId,RuntimeMetrics>, runtime_edges: &HashMap<String,RuntimeEdgeObservation>) -> Value {
    let nodes = graph.nodes().into_iter().map(|n| {
        let r = runtime_for(runtime_nodes, n);
        json!({
            "id":n.id.0,"name":n.name,"kind":kind(n.kind),"path":n.path,"line":n.line,"column":n.column,"metadata":n.metadata,
            "runtime":r.as_ref().map(|m|json!({"invocationCount":m.execution_count,"avgLatencyMs":m.avg_latency_ms,"errorRate":m.error_rate,"isHotpath":m.is_hotpath})),
            "intelligence":{"kind":if r.is_some(){"runtime"}else{"static"},"confidence":1.0,
                "evidence":[{"source":"tree-sitter-ast","ref":format!("{}:{}:{}",n.path.to_string_lossy(),n.line,n.column)}],
                "explanation":if r.is_some(){"Source symbol with observed telemetry overlay."}else{"Source symbol discovered from AST analysis."}}
        })
    }).collect::<Vec<_>>();
    let links = graph.edges().into_iter().map(|e| {
        let k = format!("{}->{}", e.from.0, e.to.0);
        let r = runtime_edges.get(&k);
        json!({
            "id":e.id,"source":e.from.0,"target":e.to.0,"kind":kind(e.kind),"weight":e.weight,"metadata":e.metadata,
            "runtime":r.map(|x|json!({"invocationCount":x.invocation_count,"avgLatencyMs":x.avg_latency_ms(),"errorRate":x.error_rate(),"lastSeenUnixNano":x.last_seen_unix_nano,"traceId":x.trace_id})),
            "intelligence":{"kind":if r.is_some(){"runtime"}else{"static"},"confidence":1.0,
                "evidence":[{"source":if r.is_some(){"otlp+ast"}else{"ast-graph"},"ref":k}]}
        })
    }).collect::<Vec<_>>();
    json!({"nodes":nodes,"links":links})
}

async fn graph_api(State(state): State<AppState>, Query(q): Query<HashMap<String,String>>) -> Result<Json<Value>, (StatusCode,String)> {
    let session = load_session(&state, &key(q.get("repo").map(String::as_str), q.get("project_id").map(String::as_str))).await;
    let graph = session.graph.read().await;
    let graph = graph.as_ref().ok_or((StatusCode::PRECONDITION_REQUIRED, "No scan has been run for this project".into()))?;
    let rn = session.runtime_nodes.read().await.clone();
    let re = session.runtime_edges.read().await.clone();
    let snapshot = session.report.read().await.as_ref().map(|r| r.snapshot_id.clone()).unwrap_or_default();
    Ok(Json(json!({
        "graph":graph_json(graph,&rn,&re),
        "snapshotId":snapshot,
        "projectKey":session.project_key,
        "generatedAt":chrono::Utc::now().to_rfc3339(),
        "persistence":"durable",
        "synthetic":false
    })))
}

fn scalar(v: &Value) -> Option<String> {
    v.as_str().map(str::to_string)
        .or_else(|| v.get("stringValue").and_then(Value::as_str).map(str::to_string))
        .or_else(|| v.get("intValue").and_then(|x| x.as_str().map(str::to_string).or_else(|| x.as_u64().map(|n|n.to_string()))))
}
fn attrs(v: Option<&Value>) -> HashMap<String,String> {
    let mut out = HashMap::new();
    if let Some(a) = v.and_then(Value::as_array) {
        for item in a {
            if let (Some(k), Some(v)) = (item.get("key").and_then(Value::as_str), item.get("value").and_then(scalar)) { out.insert(k.into(), v); }
        }
    } else if let Some(m) = v.and_then(Value::as_object) {
        for (k,v) in m { if let Some(v) = scalar(v) { out.insert(k.clone(),v); } }
    }
    out
}
fn u64v(v: Option<&Value>) -> u64 {
    v.and_then(Value::as_u64).or_else(|| v.and_then(Value::as_str).and_then(|s|s.parse().ok())).unwrap_or(0)
}
fn spans(root: &Value) -> Vec<Value> {
    if let Some(a) = root.as_array() { return a.clone(); }
    let mut out = Vec::new();
    if let Some(rs) = root.get("resourceSpans").and_then(Value::as_array) {
        for r in rs {
            let resource_attrs = attrs(r.get("resource").and_then(|x|x.get("attributes")));
            if let Some(scopes) = r.get("scopeSpans").or_else(||r.get("instrumentationLibrarySpans")).and_then(Value::as_array) {
                for scope in scopes {
                    if let Some(items) = scope.get("spans").and_then(Value::as_array) {
                        for span in items {
                            let mut merged = span.clone();
                            if let Some(map) = merged.as_object_mut() {
                                let mut a = resource_attrs.clone();
                                a.extend(attrs(span.get("attributes")));
                                map.insert("_ckbMergedAttributes".into(), serde_json::to_value(a).unwrap_or(Value::Null));
                            }
                            out.push(merged);
                        }
                    }
                }
            }
        }
    }
    out
}
fn canonical(span: &Value) -> String {
    let a: HashMap<String,String> = span.get("_ckbMergedAttributes")
        .and_then(|v|serde_json::from_value(v.clone()).ok())
        .unwrap_or_else(||attrs(span.get("attributes")));
    let file = a.get("code.file.path").or_else(||a.get("code.filepath")).or_else(||a.get("code.file.name"));
    let fun = a.get("code.function.name").or_else(||a.get("code.function")).or_else(||a.get("function.name"));
    let ns = a.get("code.namespace").or_else(||a.get("service.name"));
    match (file,fun,ns) {
        (Some(f),Some(fun),_) => format!("{}::{}",f.replace('\\',"/"),fun),
        (Some(f),None,_) => format!("{}::file",f.replace('\\',"/")),
        (None,Some(fun),Some(ns)) => format!("{}::{}",ns,fun),
        (None,Some(fun),None) => fun.clone(),
        (None,None,Some(ns)) => format!("{}::{}",ns,span.get("name").and_then(Value::as_str).unwrap_or("span")),
        _ => span.get("name").and_then(Value::as_str).unwrap_or("span").into(),
    }
}
fn errspan(span: &Value) -> bool {
    let code = span.get("status").and_then(|v|v.get("code"));
    code.and_then(Value::as_u64).map(|n|n==2)
        .or_else(||code.and_then(Value::as_str).map(|s|matches!(s.to_ascii_uppercase().as_str(),"2"|"ERROR"|"STATUS_CODE_ERROR")))
        .unwrap_or(false)
}
fn edge_observations(raw: &str) -> anyhow::Result<HashMap<String,RuntimeEdgeObservation>> {
    let root: Value = serde_json::from_str(raw)?;
    let all = spans(&root);
    let mut ids = HashMap::new();
    for span in &all {
        let id = span.get("spanId").or_else(||span.get("span_id")).and_then(Value::as_str).unwrap_or("").to_string();
        let trace = span.get("traceId").or_else(||span.get("trace_id")).and_then(Value::as_str).unwrap_or("").to_string();
        if !id.is_empty() { ids.insert(id,(trace,canonical(span))); }
    }
    let mut out = HashMap::new();
    for span in &all {
        let parent = span.get("parentSpanId").or_else(||span.get("parent_span_id")).and_then(Value::as_str).unwrap_or("");
        let Some((parent_trace,source)) = ids.get(parent).cloned() else { continue; };
        let target = canonical(span);
        let trace = span.get("traceId").or_else(||span.get("trace_id")).and_then(Value::as_str).unwrap_or(&parent_trace).to_string();
        let start = u64v(span.get("startTimeUnixNano").or_else(||span.get("start_time_unix_nano")));
        let end = u64v(span.get("endTimeUnixNano").or_else(||span.get("end_time_unix_nano")));
        let k = format!("{}->{}",source,target);
        let e = out.entry(k).or_insert(RuntimeEdgeObservation{source,target,trace_id:trace,invocation_count:0,error_count:0,total_latency_ms:0.0,last_seen_unix_nano:0});
        e.invocation_count += 1;
        e.total_latency_ms += end.saturating_sub(start) as f64 / 1_000_000.0;
        e.last_seen_unix_nano = e.last_seen_unix_nano.max(end);
        if errspan(span) { e.error_count += 1; }
    }
    Ok(out)
}
fn merge_nodes(target: &mut HashMap<NodeId,RuntimeMetrics>, incoming: HashMap<NodeId,RuntimeMetrics>) {
    for (id,m) in incoming {
        let e = target.entry(id).or_insert(RuntimeMetrics{execution_count:0,avg_latency_ms:0.0,error_rate:0.0,is_hotpath:false});
        let old = e.execution_count;
        let total = old.saturating_add(m.execution_count);
        if total > 0 {
            e.avg_latency_ms = ((e.avg_latency_ms as f64*old as f64 + m.avg_latency_ms as f64*m.execution_count as f64)/total as f64) as f32;
            e.error_rate = ((e.error_rate as f64*old as f64 + m.error_rate as f64*m.execution_count as f64)/total as f64) as f32;
        }
        e.execution_count = total;
        e.is_hotpath = e.is_hotpath || m.is_hotpath || total > 500;
    }
}
fn merge_edges(target: &mut HashMap<String,RuntimeEdgeObservation>, incoming: HashMap<String,RuntimeEdgeObservation>) {
    for (k,m) in incoming {
        if let Some(e) = target.get_mut(&k) {
            e.invocation_count += m.invocation_count;
            e.error_count += m.error_count;
            e.total_latency_ms += m.total_latency_ms;
            e.last_seen_unix_nano = e.last_seen_unix_nano.max(m.last_seen_unix_nano);
            e.trace_id = m.trace_id;
        } else { target.insert(k,m); }
    }
}

async fn otlp(State(state): State<AppState>, Json(req): Json<OtlpRequest>) -> Result<Json<Value>, (StatusCode,String)> {
    let session = load_session(&state, &key(req.repo_name.as_deref(),req.project_id.as_deref())).await;
    let raw = if let Some(v) = req.raw_spans.or(req.otlp_json) { v }
        else if let Some(v) = req.payload { serde_json::to_string(&v).map_err(internal)? }
        else { return Err((StatusCode::BAD_REQUEST,"Provide raw_spans, otlp_json, or payload".into())); };
    let nodes = OtlpReceiver::ingest_spans(&raw).map_err(internal)?;
    let edges = edge_observations(&raw).map_err(internal)?;
    let summary = OtlpReceiver::summarize(&nodes);
    merge_nodes(&mut session.runtime_nodes.write().await,nodes);
    merge_edges(&mut session.runtime_edges.write().await,edges);
    persist_session(&state,&session).await.map_err(internal)?;
    Ok(Json(json!({
        "status":"observed","kind":"runtime","spansIngested":summary.spans_ingested,"nodesUpdated":summary.nodes_updated,
        "errorSpans":summary.error_spans,"hotpathNodes":summary.hotpath_nodes,"runtimeEdges":session.runtime_edges.read().await.len(),
        "persistence":"durable","evidence":[{"source":"otlp","ref":"ingested-payload"}],"synthetic":false
    })))
}

async fn runtime(State(state): State<AppState>, Query(q): Query<HashMap<String,String>>) -> Json<Value> {
    let session = load_session(&state,&key(q.get("repo").map(String::as_str),q.get("project_id").map(String::as_str))).await;
    let nodes = session.runtime_nodes.read().await.iter().map(|(id,m)|json!({
        "id":id.0,"invocationCount":m.execution_count,"avgLatencyMs":m.avg_latency_ms,"errorRate":m.error_rate,"isHotpath":m.is_hotpath,
        "kind":"runtime","evidence":[{"source":"otlp","ref":id.0}]
    })).collect::<Vec<_>>();
    let edges = session.runtime_edges.read().await.values().map(|r|json!({
        "source":r.source,"target":r.target,"traceId":r.trace_id,"invocationCount":r.invocation_count,"avgLatencyMs":r.avg_latency_ms(),
        "errorRate":r.error_rate(),"lastSeenUnixNano":r.last_seen_unix_nano,"kind":"runtime",
        "evidence":[{"source":"otlp-parent-child","ref":format!("{}->{}",r.source,r.target)}]
    })).collect::<Vec<_>>();
    Json(json!({"observed":!nodes.is_empty()||!edges.is_empty(),"nodes":nodes,"edges":edges,"persistence":"durable","synthetic":false}))
}

async fn traces(State(state): State<AppState>, Query(q): Query<HashMap<String,String>>) -> Json<Value> {
    let session = load_session(&state,&key(q.get("repo").map(String::as_str),q.get("project_id").map(String::as_str))).await;
    let edges = session.runtime_edges.read().await;
    let mut traces: HashMap<String,Vec<Value>> = HashMap::new();
    for x in edges.values() {
        traces.entry(x.trace_id.clone()).or_default().push(json!({
            "source":x.source,"target":x.target,"invocationCount":x.invocation_count,"avgLatencyMs":x.avg_latency_ms(),
            "errorRate":x.error_rate(),"lastSeenUnixNano":x.last_seen_unix_nano
        }));
    }
    Json(json!({"kind":"runtime","observed":!edges.is_empty(),"traces":traces,"synthetic":false}))
}

async fn source(State(state): State<AppState>, Query(q): Query<HashMap<String,String>>) -> Result<Json<Value>, (StatusCode,String)> {
    let session = load_session(&state,&key(q.get("repo").map(String::as_str),q.get("project_id").map(String::as_str))).await;
    let id = q.get("node_id").ok_or((StatusCode::BAD_REQUEST,"node_id is required".into()))?;
    let graph = session.graph.read().await;
    let graph = graph.as_ref().ok_or((StatusCode::PRECONDITION_REQUIRED,"No scan".into()))?;
    let n = graph.nodes().into_iter().find(|n|n.id.0==*id).ok_or((StatusCode::NOT_FOUND,"Node not found".into()))?;
    Ok(Json(json!({
        "id":n.id.0,"name":n.name,"kind":kind(n.kind),"path":n.path,"line":n.line,"column":n.column,
        "span":{"startLine":n.metadata.get("start_line"),"startColumn":n.metadata.get("start_column"),"endLine":n.metadata.get("end_line"),
            "endColumn":n.metadata.get("end_column"),"byteStart":n.metadata.get("byte_start"),"byteEnd":n.metadata.get("byte_end")},
        "kindOfEvidence":"static","confidence":1.0,"evidence":[{"source":"tree-sitter-ast","ref":n.id.0}],"synthetic":false
    })))
}

async fn github_history(state: &AppState, repo_url: &str, max: usize) -> anyhow::Result<Value> {
    let (owner,repo) = parse_github_url(repo_url).ok_or_else(||anyhow::anyhow!("Invalid persisted GitHub URL"))?;
    let url = format!("https://api.github.com/repos/{}/{}/commits?per_page={}",owner,repo,max.min(100));
    let response = state.http.get(url)
        .header("Accept","application/vnd.github+json")
        .header("User-Agent","CKB-Software-Reality-v4")
        .header("X-GitHub-Api-Version","2022-11-28")
        .send().await?;
    if !response.status().is_success() { anyhow::bail!("GitHub history unavailable without repository access"); }
    let values: Vec<Value> = response.json().await?;
    let entries = values.into_iter().map(|v|json!({
        "commit_hash":v.get("sha").and_then(Value::as_str).unwrap_or(""),
        "author":v.pointer("/commit/author/name").and_then(Value::as_str).unwrap_or(""),
        "date":v.pointer("/commit/author/date").and_then(Value::as_str).unwrap_or(""),
        "message":v.pointer("/commit/message").and_then(Value::as_str).unwrap_or(""),
        "files_changed":[],"additions":0,"deletions":0,
        "estimated_violations_introduced":0,"risk_score":0.0
    })).collect::<Vec<_>>();
    Ok(json!({"entries":entries,"commits_analyzed":entries.len(),"source":"github-public-commit-api"}))
}

async fn history(State(state): State<AppState>, Query(q): Query<HashMap<String,String>>) -> Result<Json<Value>, (StatusCode,String)> {
    let session = load_session(&state,&key(q.get("repo").map(String::as_str),q.get("project_id").map(String::as_str))).await;
    let max = q.get("max_commits").and_then(|v|v.parse::<usize>().ok()).unwrap_or(50).min(500);
    if let Some(path) = session.repo_path.read().await.clone().or_else(||q.get("path").cloned()) {
        let timeline = GitDriftAnalyzer::build_timeline(&path,max).map_err(internal)?;
        return Ok(Json(json!({"kind":"static","source":"git","timeline":timeline,"evidence":[{"source":"git-history","ref":path}],"synthetic":false})));
    }
    if let Some(url) = session.repo_url.read().await.clone() {
        let timeline = github_history(&state,&url,max).await.map_err(internal)?;
        return Ok(Json(json!({"kind":"static","source":"github-api","timeline":timeline,"evidence":[{"source":"github-commit-history","ref":url}],"synthetic":false})));
    }
    Err((StatusCode::PRECONDITION_REQUIRED,"No repository history source is attached to this project".into()))
}

async fn snapshots(State(state): State<AppState>, Query(q): Query<HashMap<String,String>>) -> Result<Json<Value>, (StatusCode,String)> {
    let project_key = key(q.get("repo").map(String::as_str),q.get("project_id").map(String::as_str));
    let dir = snapshot_dir(&state,&project_key);
    let mut out = Vec::new();
    if let Ok(mut rd) = tokio::fs::read_dir(dir).await {
        while let Ok(Some(e)) = rd.next_entry().await {
            if let Ok(bytes) = tokio::fs::read(e.path()).await {
                if let Ok(s) = bincode::deserialize::<ArchitectureSnapshot>(&bytes) {
                    out.push(json!({"id":s.id,"createdAt":s.created_at,"nodes":s.report.nodes,"edges":s.report.edges,
                        "filesProcessed":s.report.files_processed,"violations":s.report.drift.len(),"durationMs":s.report.duration_ms}));
                }
            }
        }
    }
    out.sort_by(|a,b|a.get("createdAt").and_then(Value::as_str).cmp(&b.get("createdAt").and_then(Value::as_str)));
    Ok(Json(json!({"projectKey":project_key,"snapshots":out,"source":"ckb-persistent-snapshots","synthetic":false})))
}

async fn snapshot_graph(State(state): State<AppState>, Query(q): Query<HashMap<String,String>>) -> Result<Json<Value>, (StatusCode,String)> {
    let project_key = key(q.get("repo").map(String::as_str),q.get("project_id").map(String::as_str));
    let id = q.get("snapshot_id").ok_or((StatusCode::BAD_REQUEST,"snapshot_id is required".into()))?;
    let snapshot = read_snapshot(&state,&project_key,id).await.map_err(internal)?;
    Ok(Json(json!({
        "kind":"static","source":"persistent-snapshot","snapshotId":snapshot.id,"createdAt":snapshot.created_at,
        "graph":graph_json(&snapshot.graph,&HashMap::new(),&HashMap::new()),"report":snapshot.report,"synthetic":false
    })))
}

async fn diff(State(state): State<AppState>, Json(req): Json<DiffRequest>) -> Result<Json<Value>, (StatusCode,String)> {
    let project_key = key(req.repo_name.as_deref(),req.project_id.as_deref());
    let a = read_snapshot(&state,&project_key,&req.from_snapshot).await.map_err(internal)?;
    let b = read_snapshot(&state,&project_key,&req.to_snapshot).await.map_err(internal)?;
    let an:HashSet<String> = a.graph.nodes().into_iter().map(|n|n.id.0.clone()).collect();
    let bn:HashSet<String> = b.graph.nodes().into_iter().map(|n|n.id.0.clone()).collect();
    let ae:HashSet<String> = a.graph.edges().into_iter().map(|e|format!("{}->{}/{}",e.from.0,e.to.0,kind(e.kind))).collect();
    let be:HashSet<String> = b.graph.edges().into_iter().map(|e|format!("{}->{}/{}",e.from.0,e.to.0,kind(e.kind))).collect();
    Ok(Json(json!({
        "kind":"static","source":"persistent-snapshot-diff","from":req.from_snapshot,"to":req.to_snapshot,
        "addedNodes":bn.difference(&an).cloned().collect::<Vec<_>>(),"removedNodes":an.difference(&bn).cloned().collect::<Vec<_>>(),
        "addedEdges":be.difference(&ae).cloned().collect::<Vec<_>>(),"removedEdges":ae.difference(&be).cloned().collect::<Vec<_>>(),
        "summary":{"nodeDelta":bn.len() as i64-an.len() as i64,"edgeDelta":be.len() as i64-ae.len() as i64,
            "fromViolations":a.report.drift.len(),"toViolations":b.report.drift.len()},
        "evidence":[{"source":"ckb-snapshot","ref":a.id},{"source":"ckb-snapshot","ref":b.id}],"synthetic":false
    })))
}

async fn causal_path(State(state): State<AppState>, Json(req): Json<CausalPathRequest>) -> Result<Json<Value>, (StatusCode,String)> {
    let session = load_session(&state,&key(req.repo_name.as_deref(),req.project_id.as_deref())).await;
    let graph = session.graph.read().await;
    let graph = graph.as_ref().ok_or((StatusCode::PRECONDITION_REQUIRED,"No scan".into()))?;
    let mut report = CausalArchitectureEngine::shortest_path(graph,&NodeId(req.source),&NodeId(req.target),req.max_depth.unwrap_or(12)).map_err(internal)?;
    let runtime = session.runtime_nodes.read().await;
    for step in &mut report.steps {
        step.runtime_observed_at_from = step.runtime_observed_at_from || runtime.keys().any(|id| id.0 == step.from);
        step.runtime_observed_at_to = step.runtime_observed_at_to || runtime.keys().any(|id| id.0 == step.to);
    }
    Ok(Json(serde_json::to_value(report).map_err(internal)?))
}

async fn failure_cone(State(state): State<AppState>, Json(req): Json<FailureConeRequest>) -> Result<Json<Value>, (StatusCode,String)> {
    let session = load_session(&state,&key(req.repo_name.as_deref(),req.project_id.as_deref())).await;
    let graph = session.graph.read().await;
    let graph = graph.as_ref().ok_or((StatusCode::PRECONDITION_REQUIRED,"No scan".into()))?;
    let mut report = CausalArchitectureEngine::failure_cone(graph,&NodeId(req.root),req.max_depth.unwrap_or(12)).map_err(internal)?;
    let runtime = session.runtime_nodes.read().await;
    for node in &mut report.affected { node.runtime_observed = node.runtime_observed || runtime.keys().any(|id| id.0 == node.id); }
    Ok(Json(serde_json::to_value(report).map_err(internal)?))
}

async fn memory_query(State(state): State<AppState>, Json(req): Json<MemoryQueryRequest>) -> Result<Json<Value>, (StatusCode,String)> {
    let session = load_session(&state,&key(req.repo_name.as_deref(),req.project_id.as_deref())).await;
    let graph = session.graph.read().await;
    let graph = graph.as_ref().ok_or((StatusCode::PRECONDITION_REQUIRED,"No scan".into()))?;
    let report = ArchitectureMemoryEngine::query(graph,&req.query,req.depth.unwrap_or(2),req.limit.unwrap_or(12)).map_err(internal)?;
    Ok(Json(serde_json::to_value(report).map_err(internal)?))
}

async fn code_dna(State(state): State<AppState>, Query(q): Query<HashMap<String,String>>) -> Result<Json<Value>, (StatusCode,String)> {
    let session = load_session(&state,&key(q.get("repo").map(String::as_str),q.get("project_id").map(String::as_str))).await;
    let graph = session.graph.read().await;
    let graph = graph.as_ref().ok_or((StatusCode::PRECONDITION_REQUIRED,"No scan".into()))?;
    let report = ArchitectureMemoryEngine::code_dna(graph).map_err(internal)?;
    Ok(Json(serde_json::to_value(report).map_err(internal)?))
}

async fn test_gaps(State(state): State<AppState>, Query(q): Query<HashMap<String,String>>) -> Result<Json<Value>, (StatusCode,String)> {
    let session=load_session(&state,&key(q.get("repo").map(String::as_str),q.get("project_id").map(String::as_str))).await;
    let graph=session.graph.read().await;
    let graph=graph.as_ref().ok_or((StatusCode::PRECONDITION_REQUIRED,"No scan".into()))?;
    Ok(Json(serde_json::to_value(TestCoverageAnalyzer::analyze_gaps(graph).map_err(internal)?).map_err(internal)?))
}
async fn rules(State(state): State<AppState>, Query(q): Query<HashMap<String,String>>) -> Result<String,(StatusCode,String)> {
    let session=load_session(&state,&key(q.get("repo").map(String::as_str),q.get("project_id").map(String::as_str))).await;
    let graph=session.graph.read().await;
    let graph=graph.as_ref().ok_or((StatusCode::PRECONDITION_REQUIRED,"No scan".into()))?;
    ArchitectureAnalyzer::new().generate_ai_guidelines(graph).map_err(internal)
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt::init();
    let data_dir = PathBuf::from(std::env::var("CKB_REALITY_DATA_DIR").unwrap_or_else(|_|"./ckb_reality_data".into()));
    tokio::fs::create_dir_all(&data_dir).await?;
    let http = reqwest::Client::builder().timeout(std::time::Duration::from_secs(120)).build()?;
    let state = AppState {
        sessions:Arc::new(RwLock::new(HashMap::new())),
        api_key:std::env::var("CKB_API_KEY").ok().filter(|v|!v.is_empty()).map(Arc::new),
        data_dir:Arc::new(data_dir),
        http,
    };
    if state.api_key.is_none() { warn!("CKB_API_KEY is not configured; Reality API is unauthenticated"); }

    let protected = Router::new()
        .route("/api/v1/scan",post(scan))
        .route("/api/v1/report",get(report))
        .route("/api/v1/impact",post(impact))
        .route("/api/v1/otlp",post(otlp))
        .route("/api/v1/drift-timeline",get(history))
        .route("/api/v1/test-gaps",get(test_gaps))
        .route("/api/v1/rules",get(rules))
        .route("/api/v1/intelligence/scan/github",post(scan_github))
        .route("/api/v1/intelligence/scan/zip",post(scan_zip))
        .route("/api/v1/intelligence/graph",get(graph_api))
        .route("/api/v1/intelligence/source",get(source))
        .route("/api/v1/intelligence/runtime",get(runtime))
        .route("/api/v1/intelligence/traces",get(traces))
        .route("/api/v1/intelligence/impact",post(impact))
        .route("/api/v1/intelligence/telemetry/otlp",post(otlp))
        .route("/api/v1/intelligence/history",get(history))
        .route("/api/v1/intelligence/snapshots",get(snapshots))
        .route("/api/v1/intelligence/snapshot",get(snapshot_graph))
        .route("/api/v1/intelligence/diff",post(diff))
        .route("/api/v1/intelligence/causal-path",post(causal_path))
        .route("/api/v1/intelligence/failure-cone",post(failure_cone))
        .route("/api/v1/intelligence/memory/query",post(memory_query))
        .route("/api/v1/intelligence/code-dna",get(code_dna))
        .route_layer(middleware::from_fn_with_state(state.clone(),auth));

    let cors = match std::env::var("CKB_ALLOWED_ORIGIN") {
        Ok(v) if v != "*" => CorsLayer::new().allow_origin(v.parse::<axum::http::HeaderValue>()?).allow_methods(Any).allow_headers(Any),
        _ => CorsLayer::new().allow_origin(Any).allow_methods(Any).allow_headers(Any),
    };
    let app = Router::new()
        .route("/health",get(health))
        .merge(protected)
        .layer(DefaultBodyLimit::max(85 * 1024 * 1024))
        .layer(cors)
        .with_state(state);

    let port=std::env::var("PORT").ok().and_then(|v|v.parse().ok()).unwrap_or(3000);
    let all=std::env::var("CKB_BIND_ALL").map(|v|v=="1"||v.eq_ignore_ascii_case("true")).unwrap_or(false);
    let host=if all{[0,0,0,0]}else{[127,0,0,1]};
    let addr=std::net::SocketAddr::from((host,port));
    info!("CKB Reality API v4 listening on {}",addr);
    let listener=tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener,app).await?;
    Ok(())
}
