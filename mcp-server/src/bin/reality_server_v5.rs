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
    io::{Cursor, Write},
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

#[derive(Clone, Serialize, Deserialize)]
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
        if self.invocation_count == 0 {
            0.0
        } else {
            self.error_count as f64 / self.invocation_count as f64
        }
    }

    fn avg_latency_ms(&self) -> f64 {
        if self.invocation_count == 0 {
            0.0
        } else {
            self.total_latency_ms / self.invocation_count as f64
        }
    }
}

#[derive(Serialize, Deserialize)]
struct PersistedSession {
    graph: Option<DependencyGraph>,
    report: Option<ScanReport>,
    repo_path: Option<String>,
    repo_url: Option<String>,
    runtime_nodes: HashMap<NodeId, RuntimeMetrics>,
    runtime_edges: HashMap<String, RuntimeEdgeObservation>,
    saved_at: String,
}

#[derive(Serialize, Deserialize)]
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

#[derive(Deserialize)]
struct ScanRequest {
    path: String,
    repo_name: Option<String>,
    project_id: Option<String>,
}

#[derive(Deserialize)]
struct GitHubScanRequest {
    github_url: String,
    github_token: Option<String>,
    project_id: Option<String>,
    repo_name: Option<String>,
}

#[derive(Deserialize)]
struct ZipScanRequest {
    file_data: String,
    file_name: Option<String>,
    project_id: Option<String>,
    repo_name: Option<String>,
}

#[derive(Deserialize)]
struct ImpactRequest {
    path: Option<String>,
    file: String,
    #[serde(default = "default_line")]
    line: u32,
    change_type: Option<String>,
    repo_name: Option<String>,
    project_id: Option<String>,
}

#[derive(Deserialize)]
struct OtlpRequest {
    raw_spans: Option<String>,
    otlp_json: Option<String>,
    payload: Option<Value>,
    repo_name: Option<String>,
    project_id: Option<String>,
}

#[derive(Deserialize)]
struct DiffRequest {
    from_snapshot: String,
    to_snapshot: String,
    project_id: Option<String>,
    repo_name: Option<String>,
}

#[derive(Deserialize)]
struct CausalPathRequest {
    source: String,
    target: String,
    max_depth: Option<usize>,
    project_id: Option<String>,
    repo_name: Option<String>,
}

#[derive(Deserialize)]
struct FailureConeRequest {
    root: String,
    max_depth: Option<usize>,
    project_id: Option<String>,
    repo_name: Option<String>,
}

#[derive(Deserialize)]
struct MemoryQueryRequest {
    query: String,
    depth: Option<usize>,
    limit: Option<usize>,
    project_id: Option<String>,
    repo_name: Option<String>,
}

fn default_line() -> u32 {
    1
}

fn internal<E: std::fmt::Display>(error: E) -> (StatusCode, String) {
    (StatusCode::INTERNAL_SERVER_ERROR, error.to_string())
}

fn bad<E: std::fmt::Display>(error: E) -> (StatusCode, String) {
    (StatusCode::BAD_REQUEST, error.to_string())
}

fn project_key(repo: Option<&str>, project: Option<&str>) -> String {
    project
        .filter(|value| !value.is_empty())
        .or_else(|| repo.filter(|value| !value.is_empty()))
        .unwrap_or("default")
        .to_string()
}

fn safe_key(value: &str) -> String {
    value
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect()
}

fn session_file(state: &AppState, key: &str) -> PathBuf {
    state
        .data_dir
        .join("sessions")
        .join(format!("{}.bin", safe_key(key)))
}

fn snapshot_dir(state: &AppState, key: &str) -> PathBuf {
    state.data_dir.join("snapshots").join(safe_key(key))
}

async fn load_session(state: &AppState, key: &str) -> Session {
    let existing = {
        let sessions = state.sessions.read().await;
        sessions.get(key).cloned()
    };
    if let Some(session) = existing {
        return session;
    }

    let session = Session::empty(key.to_string());
    if let Ok(bytes) = tokio::fs::read(session_file(state, key)).await {
        if let Ok(persisted) = bincode::deserialize::<PersistedSession>(&bytes) {
            *session.graph.write().await = persisted.graph;
            *session.report.write().await = persisted.report;
            *session.repo_path.write().await = persisted.repo_path;
            *session.repo_url.write().await = persisted.repo_url;
            *session.runtime_nodes.write().await = persisted.runtime_nodes;
            *session.runtime_edges.write().await = persisted.runtime_edges;
        }
    }

    state
        .sessions
        .write()
        .await
        .insert(key.to_string(), session.clone());
    session
}

async fn persist_session(state: &AppState, session: &Session) -> anyhow::Result<()> {
    let graph = session.graph.read().await.clone();
    let report = session.report.read().await.clone();
    let repo_path = session.repo_path.read().await.clone();
    let repo_url = session.repo_url.read().await.clone();
    let runtime_nodes = session.runtime_nodes.read().await.clone();
    let runtime_edges = session.runtime_edges.read().await.clone();

    let persisted = PersistedSession {
        graph,
        report,
        repo_path,
        repo_url,
        runtime_nodes,
        runtime_edges,
        saved_at: chrono::Utc::now().to_rfc3339(),
    };

    let path = session_file(state, &session.project_key);
    if let Some(parent) = path.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }
    let temp = path.with_extension("bin.tmp");
    tokio::fs::write(&temp, bincode::serialize(&persisted)?).await?;
    tokio::fs::rename(&temp, &path).await?;
    Ok(())
}

/// Rewrite a scan report copy for snapshot storage: drift endpoints
/// are node IDs and rule messages quote file paths, so the same
/// scan-root prefix the graph relativization strips has to come out of
/// them too — otherwise the stored snapshot's report disagrees with its
/// own graph about node identity. Only the `<root>/` prefix is removed;
/// the remainder of every message is the engine's own text, identical
/// to what a CLI scan invoked at the project root produces.
fn relativize_report(report: &mut ScanReport, root: &str) {
    let root = root.trim_end_matches('/');
    if root.is_empty() || root == "/" {
        return;
    }
    let prefix = format!("{root}/");
    let strip = |value: &str| value.replace(&prefix, "");
    for violation in &mut report.drift {
        violation.from.0 = strip(&violation.from.0);
        violation.to.0 = strip(&violation.to.0);
        violation.message = strip(&violation.message);
        if let Some(fix) = &mut violation.suggested_fix {
            *fix = strip(fix);
        }
    }
}

/// Persist a snapshot of a completed scan. The caller passes the
/// project-relative copies (`DependencyGraph::relative_to` + the
/// relativized report): snapshots are the durable cross-scan identity,
/// so they must not carry the per-scan extraction root
/// (`/tmp/ckb-zip-<uuid>/…`, `ckb-reality-<uuid>/…`) in their node IDs —
/// with the root baked in, two scans of the same tree share no node
/// identity and every diff degenerates to all-added/all-removed. The
/// live session keeps the rooted graph (in-flight endpoints resolve
/// real files with it); only the stored snapshot is relativized.
async fn persist_snapshot(
    state: &AppState,
    session: &Session,
    graph: &DependencyGraph,
    report: &ScanReport,
) -> anyhow::Result<Option<String>> {
    let id = report.snapshot_id.clone();
    let snapshot = ArchitectureSnapshot {
        id: id.clone(),
        project_key: session.project_key.clone(),
        created_at: chrono::Utc::now().to_rfc3339(),
        report: report.clone(),
        graph: graph.clone(),
    };

    let directory = snapshot_dir(state, &session.project_key);
    tokio::fs::create_dir_all(&directory).await?;
    let path = directory.join(format!("{}.bin", safe_key(&id)));
    tokio::fs::write(path, bincode::serialize(&snapshot)?).await?;
    Ok(Some(id))
}

async fn read_snapshot(
    state: &AppState,
    key: &str,
    id: &str,
) -> anyhow::Result<ArchitectureSnapshot> {
    let path = snapshot_dir(state, key).join(format!("{}.bin", safe_key(id)));
    let bytes = tokio::fs::read(path).await?;
    Ok(bincode::deserialize(&bytes)?)
}

fn extract_api_key(headers: &HeaderMap) -> Option<String> {
    headers
        .get("x-api-key")
        .and_then(|value| value.to_str().ok())
        .map(str::to_string)
        .or_else(|| {
            headers
                .get(axum::http::header::AUTHORIZATION)
                .and_then(|value| value.to_str().ok())
                .and_then(|value| value.strip_prefix("Bearer "))
                .map(str::to_string)
        })
}

async fn auth(
    State(state): State<AppState>,
    headers: HeaderMap,
    request: axum::http::Request<axum::body::Body>,
    next: Next,
) -> Result<Response, (StatusCode, String)> {
    if let Some(expected) = &state.api_key {
        if extract_api_key(&headers).as_deref() != Some(expected.as_str()) {
            return Err((
                StatusCode::UNAUTHORIZED,
                "Missing or invalid CKB API key".into(),
            ));
        }
    }
    Ok(next.run(request).await)
}

fn supported(path: &Path) -> bool {
    matches!(
        path.extension().and_then(|value| value.to_str()).unwrap_or(""),
        "ts" | "tsx" | "js" | "jsx" | "mjs" | "py" | "go" | "rs" | "java"
    )
}

fn discover(root: &str) -> anyhow::Result<Vec<PathBuf>> {
    let mut output = Vec::new();
    let mut stack = vec![PathBuf::from(root)];
    while let Some(directory) = stack.pop() {
        for entry in std::fs::read_dir(&directory)? {
            let entry = entry?;
            let path = entry.path();
            let name = path.file_name().and_then(|value| value.to_str()).unwrap_or("");
            if path.is_dir() {
                if !matches!(
                    name,
                    ".git"
                        | "node_modules"
                        | "target"
                        | "dist"
                        | "build"
                        | ".next"
                        | "vendor"
                        | "coverage"
                        | ".turbo"
                        | ".yarn"
                ) {
                    stack.push(path);
                }
            } else if supported(&path) {
                output.push(path);
            }
        }
    }
    output.sort();
    Ok(output)
}

fn package_identity(root: &str) -> Option<String> {
    let root_path = Path::new(root);
    if let Ok(content) = std::fs::read_to_string(root_path.join("package.json")) {
        if let Ok(value) = serde_json::from_str::<Value>(&content) {
            if let Some(name) = value.get("name").and_then(Value::as_str) {
                return Some(name.to_string());
            }
        }
    }
    if let Ok(content) = std::fs::read_to_string(root_path.join("go.mod")) {
        for line in content.lines() {
            if let Some(value) = line.trim().strip_prefix("module ") {
                return Some(value.trim().to_string());
            }
        }
    }
    for file in ["Cargo.toml", "pyproject.toml"] {
        if let Ok(content) = std::fs::read_to_string(root_path.join(file)) {
            for line in content.lines() {
                let line = line.trim();
                if let Some(value) = line
                    .strip_prefix("name")
                    .and_then(|value| value.trim_start().strip_prefix('='))
                {
                    let name = value.trim().trim_matches('"');
                    if !name.is_empty() {
                        return Some(name.to_string());
                    }
                }
            }
        }
    }
    None
}

fn external_dependencies(analyses: &[FileAnalysis]) -> Vec<String> {
    let mut dependencies = BTreeSet::new();
    for analysis in analyses {
        for import in &analysis.imports {
            if import.source.starts_with('.')
                || import.source.starts_with('/')
                || import.source.is_empty()
            {
                continue;
            }
            let dependency = if let Some(scoped) = import.source.strip_prefix('@') {
                let mut parts = scoped.split('/');
                match (parts.next(), parts.next()) {
                    (Some(scope), Some(package)) => format!("@{}/{}", scope, package),
                    _ => import.source.clone(),
                }
            } else {
                import
                    .source
                    .split('/')
                    .next()
                    .unwrap_or(&import.source)
                    .to_string()
            };
            dependencies.insert(dependency);
        }
    }
    dependencies.into_iter().collect()
}

/// Merge `.ckb/rules.json` violations into a scan's drift — the same
/// enforcement `CkbEngine::scan_codebase` runs for the CLI/MCP paths
/// (`detect_drift_with_project_rules` in ckb-core). Until now this
/// hosted path called `detect_drift` alone, so engine reports never
/// contained ForbiddenDependency violations even for projects carrying
/// a valid rules file: Mission Control (via the cloud backend's own
/// analyzer merge) showed them, but anything reading the engine report
/// itself — the Raiziom console's health among it — silently missed
/// them and quoted a higher health score.
///
/// Semantics deliberately mirror the core: violations come from
/// `check_project_rules` unchanged (same kind/boundary/severity/message
/// shape), are deduplicated against the inferred drift with the core's
/// `kind|from|to|boundary` key, and a malformed rules file errors the
/// scan — `load_project_rules` fails loudly and every caller of
/// `build_graph` already propagates scan errors as a failed request,
/// exactly as the CLI fails the scan. Silently skipping a broken rules
/// file would leave the project unenforced while its owners believe
/// otherwise.
fn apply_project_rules(
    graph: &DependencyGraph,
    root: &str,
    drift: &mut Vec<ckb_core::DriftViolation>,
) -> anyhow::Result<()> {
    if let Some(rules) = ckb_core::load_project_rules(Path::new(root))? {
        let mut seen: HashSet<String> = drift
            .iter()
            .map(|v| format!("{:?}|{}|{}|{}", v.kind, v.from.0, v.to.0, v.boundary))
            .collect();
        for violation in ckb_core::check_project_rules(graph, &rules) {
            let key = format!(
                "{:?}|{}|{}|{}",
                violation.kind, violation.from.0, violation.to.0, violation.boundary
            );
            if seen.insert(key) {
                drift.push(violation);
            }
        }
    }
    Ok(())
}

async fn build_graph(root: &str) -> anyhow::Result<(DependencyGraph, ScanReport)> {
    let started = std::time::Instant::now();
    let parser = LanguageParser::new();
    let files = discover(root)?;
    let mut analyses = Vec::new();

    for file in files {
        let path = file.to_string_lossy().to_string();
        if let Ok(analysis) = parser.parse_file(&path).await {
            analyses.push(analysis);
        }
    }
    if analyses.is_empty() {
        anyhow::bail!("No supported source files could be parsed");
    }

    let mut graph = DependencyGraph::new();
    for analysis in &analyses {
        graph.add_file(analysis)?;
    }
    graph.build_call_graph()?;
    graph.build_type_graph()?;

    let analyzer = ArchitectureAnalyzer::new();
    let patterns = analyzer.detect_patterns(&graph)?;
    let mut drift = analyzer.detect_drift(&graph, &patterns)?;
    apply_project_rules(&graph, root, &mut drift)?;
    let report = ScanReport {
        files_processed: analyses.len(),
        nodes: graph.node_count(),
        edges: graph.edge_count(),
        patterns,
        drift,
        snapshot_id: uuid::Uuid::new_v4().to_string(),
        duration_ms: started.elapsed().as_secs_f64() * 1000.0,
        package_identity: package_identity(root),
        external_dependencies: external_dependencies(&analyses),
    };
    Ok((graph, report))
}

fn parse_github_url(raw: &str) -> Option<(String, String)> {
    let clean = raw.trim().trim_end_matches('/').trim_end_matches(".git");
    let marker = "github.com/";
    let start = clean.find(marker)? + marker.len();
    let mut parts = clean[start..].split('/');
    let owner = parts.next()?.trim();
    let repo = parts.next()?.trim();
    if owner.is_empty() || repo.is_empty() {
        return None;
    }
    Some((owner.to_string(), repo.to_string()))
}

fn extract_zip_safely(bytes: &[u8], target: &Path) -> anyhow::Result<PathBuf> {
    if bytes.len() > MAX_ARCHIVE_BYTES {
        anyhow::bail!("Archive exceeds {} MB limit", MAX_ARCHIVE_BYTES / 1024 / 1024);
    }
    std::fs::create_dir_all(target)?;
    let mut zip = ZipArchive::new(Cursor::new(bytes))?;
    if zip.len() > MAX_ARCHIVE_FILES {
        anyhow::bail!("Archive contains too many files");
    }

    let mut total = 0u64;
    for index in 0..zip.len() {
        let mut entry = zip.by_index(index)?;
        total = total.saturating_add(entry.size());
        if total > MAX_EXTRACTED_BYTES {
            anyhow::bail!("Expanded archive exceeds safety limit");
        }
        let Some(relative) = entry.enclosed_name().map(Path::to_path_buf) else {
            continue;
        };
        let output = target.join(relative);
        if entry.is_dir() {
            std::fs::create_dir_all(&output)?;
            continue;
        }
        if let Some(parent) = output.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let mut file = std::fs::File::create(&output)?;
        std::io::copy(&mut entry, &mut file)?;
        file.flush()?;
    }

    let mut entries = std::fs::read_dir(target)?
        .filter_map(Result::ok)
        .collect::<Vec<_>>();
    if entries.len() == 1 && entries[0].path().is_dir() {
        Ok(entries.remove(0).path())
    } else {
        Ok(target.to_path_buf())
    }
}

async fn save_scan(
    state: &AppState,
    key: String,
    graph: DependencyGraph,
    report: ScanReport,
    repo_path: Option<String>,
    repo_url: Option<String>,
    scan_root: &str,
) -> Result<Json<Value>, (StatusCode, String)> {
    let session = load_session(state, &key).await;
    // The persisted snapshot stores project-relative identity (see
    // persist_snapshot); the live session keeps the rooted graph.
    let snapshot_graph = graph.relative_to(Path::new(scan_root));
    let mut snapshot_report = report.clone();
    relativize_report(&mut snapshot_report, scan_root);
    *session.graph.write().await = Some(graph);
    *session.report.write().await = Some(report.clone());
    *session.repo_path.write().await = repo_path;
    *session.repo_url.write().await = repo_url;
    session.runtime_nodes.write().await.clear();
    session.runtime_edges.write().await.clear();
    persist_session(state, &session).await.map_err(internal)?;
    let snapshot_id = persist_snapshot(state, &session, &snapshot_graph, &snapshot_report)
        .await
        .map_err(internal)?;

    Ok(Json(json!({
        "status":"success",
        "projectKey":key,
        "filesProcessed":report.files_processed,
        "nodes":report.nodes,
        "edges":report.edges,
        "violationsFound":report.drift.len(),
        "snapshotId":snapshot_id,
        "engine":"tree-sitter-rust-reality-v5",
        "synthetic":false
    })))
}

async fn health(State(state): State<AppState>) -> Json<Value> {
    Json(json!({
        "status":"healthy",
        "service":"ckb-reality-server-v5",
        "realityApi":"v5",
        "remoteGitHubScan":true,
        "zipScan":true,
        "graphPersistence":"durable-bincode-snapshots",
        "dataDir":state.data_dir.to_string_lossy(),
        "evidencePolicy":"static-runtime-predicted-separated"
    }))
}

async fn scan(
    State(state): State<AppState>,
    Json(request): Json<ScanRequest>,
) -> Result<Json<Value>, (StatusCode, String)> {
    let key = project_key(request.repo_name.as_deref(), request.project_id.as_deref());
    let (graph, report) = build_graph(&request.path).await.map_err(internal)?;
    let scan_root = request.path.clone();
    save_scan(
        &state,
        key,
        graph,
        report,
        Some(request.path),
        None,
        &scan_root,
    )
    .await
}

async fn scan_github(
    State(state): State<AppState>,
    Json(request): Json<GitHubScanRequest>,
) -> Result<Json<Value>, (StatusCode, String)> {
    let (owner, repo) = parse_github_url(&request.github_url)
        .ok_or_else(|| bad("Invalid GitHub URL"))?;
    let archive_url = format!("https://api.github.com/repos/{owner}/{repo}/zipball/HEAD");
    let mut fetch = state
        .http
        .get(archive_url)
        .header("Accept", "application/vnd.github+json")
        .header("User-Agent", "CKB-Software-Reality-v5")
        .header("X-GitHub-Api-Version", "2022-11-28");
    if let Some(token) = request
        .github_token
        .as_deref()
        .filter(|value| !value.trim().is_empty())
    {
        fetch = fetch.bearer_auth(token.trim());
    }

    let response = fetch.send().await.map_err(internal)?;
    if response.status() == reqwest::StatusCode::NOT_FOUND {
        return Err((
            StatusCode::NOT_FOUND,
            "Repository not found or token lacks access".into(),
        ));
    }
    if response.status() == reqwest::StatusCode::UNAUTHORIZED
        || response.status() == reqwest::StatusCode::FORBIDDEN
    {
        return Err((
            StatusCode::FORBIDDEN,
            "GitHub repository access denied".into(),
        ));
    }
    if !response.status().is_success() {
        return Err((
            StatusCode::BAD_GATEWAY,
            format!("GitHub returned {}", response.status()),
        ));
    }

    let bytes = response.bytes().await.map_err(internal)?;
    let temp = std::env::temp_dir().join(format!("ckb-reality-{}", uuid::Uuid::new_v4()));
    let repo_root = extract_zip_safely(&bytes, &temp).map_err(bad)?;
    let built = build_graph(&repo_root.to_string_lossy()).await;
    let _ = std::fs::remove_dir_all(&temp);
    let (graph, report) = built.map_err(internal)?;
    let key = project_key(request.repo_name.as_deref(), request.project_id.as_deref());
    save_scan(
        &state,
        key,
        graph,
        report,
        None,
        Some(format!("https://github.com/{owner}/{repo}")),
        &repo_root.to_string_lossy(),
    )
    .await
}

async fn scan_zip(
    State(state): State<AppState>,
    Json(request): Json<ZipScanRequest>,
) -> Result<Json<Value>, (StatusCode, String)> {
    let payload = request.file_data.rsplit(',').next().unwrap_or(&request.file_data);
    let bytes = BASE64.decode(payload.as_bytes()).map_err(bad)?;
    let temp = std::env::temp_dir().join(format!("ckb-zip-{}", uuid::Uuid::new_v4()));
    let repo_root = extract_zip_safely(&bytes, &temp).map_err(bad)?;
    let built = build_graph(&repo_root.to_string_lossy()).await;
    let _ = std::fs::remove_dir_all(&temp);
    let (graph, report) = built.map_err(internal)?;
    let key = project_key(
        request.repo_name.as_deref().or(request.file_name.as_deref()),
        request.project_id.as_deref(),
    );
    save_scan(
        &state,
        key,
        graph,
        report,
        None,
        request.file_name,
        &repo_root.to_string_lossy(),
    )
    .await
}

async fn report(
    State(state): State<AppState>,
    Query(query): Query<HashMap<String, String>>,
) -> Result<Json<ScanReport>, (StatusCode, String)> {
    let key = project_key(
        query.get("repo").map(String::as_str),
        query.get("project_id").map(String::as_str),
    );
    let session = load_session(&state, &key).await;
    let value = session.report.read().await.clone();
    value
        .map(Json)
        .ok_or((StatusCode::NOT_FOUND, "No scan has been run for this project".into()))
}

fn change_type(value: Option<&str>) -> ChangeType {
    match value.unwrap_or("modify").to_ascii_lowercase().as_str() {
        "add" => ChangeType::Add,
        "delete" => ChangeType::Delete,
        "rename" => ChangeType::Rename,
        _ => ChangeType::Modify,
    }
}

fn kind<T: std::fmt::Debug>(value: T) -> String {
    format!("{:?}", value).to_ascii_lowercase()
}

async fn impact(
    State(state): State<AppState>,
    Json(request): Json<ImpactRequest>,
) -> Result<Json<Value>, (StatusCode, String)> {
    let key = project_key(request.repo_name.as_deref(), request.project_id.as_deref());
    let session = load_session(&state, &key).await;

    let missing_graph = { session.graph.read().await.is_none() };
    if missing_graph {
        let path = request.path.as_deref().ok_or((
            StatusCode::PRECONDITION_REQUIRED,
            "Scan first or provide a local path".into(),
        ))?;
        let (graph, report) = build_graph(path).await.map_err(internal)?;
        *session.graph.write().await = Some(graph);
        *session.report.write().await = Some(report);
        *session.repo_path.write().await = Some(path.to_string());
        persist_session(&state, &session).await.map_err(internal)?;
    }

    let graph_guard = session.graph.read().await;
    let graph = graph_guard.as_ref().ok_or((
        StatusCode::PRECONDITION_REQUIRED,
        "No architecture graph is available".into(),
    ))?;
    let affected = graph
        .find_affected_nodes(&request.file, request.line)
        .map_err(internal)?;
    let result = graph
        .calculate_impact(&affected, change_type(request.change_type.as_deref()))
        .map_err(internal)?;

    Ok(Json(json!({
        "kind":"predicted",
        "confidencePolicy":"derived-per-path",
        "assumptions":["Current persisted graph is the baseline"],
        "evidence":[{"source":"ast-graph","ref":format!("{}:{}",request.file,request.line)}],
        "result":result,
        "synthetic":false
    })))
}

fn runtime_for(
    runtime: &HashMap<NodeId, RuntimeMetrics>,
    node: &Node,
) -> Option<RuntimeMetrics> {
    if let Some(metrics) = runtime.get(&node.id) {
        return Some(metrics.clone());
    }
    let path = node.path.to_string_lossy().replace('\\', "/");
    runtime.iter().find_map(|(id, metrics)| {
        let raw = id.0.replace('\\', "/");
        if raw == node.name
            || raw.ends_with(&format!("::{}", node.name))
            || (raw.starts_with(&format!("{}::", path)) && raw.ends_with(&node.name))
        {
            Some(metrics.clone())
        } else {
            None
        }
    })
}

fn graph_json(
    graph: &DependencyGraph,
    runtime_nodes: &HashMap<NodeId, RuntimeMetrics>,
    runtime_edges: &HashMap<String, RuntimeEdgeObservation>,
) -> Value {
    let nodes = graph
        .nodes()
        .into_iter()
        .map(|node| {
            let runtime = runtime_for(runtime_nodes, node);
            json!({
                "id":node.id.0,
                "name":node.name,
                "kind":kind(node.kind),
                "path":node.path,
                "line":node.line,
                "column":node.column,
                "metadata":node.metadata,
                "runtime":runtime.as_ref().map(|metrics| json!({
                    "invocationCount":metrics.execution_count,
                    "avgLatencyMs":metrics.avg_latency_ms,
                    "errorRate":metrics.error_rate,
                    "isHotpath":metrics.is_hotpath
                })),
                "intelligence":{
                    "kind":if runtime.is_some(){"runtime"}else{"static"},
                    "confidence":1.0,
                    "evidence":[{"source":"tree-sitter-ast","ref":format!("{}:{}:{}",node.path.to_string_lossy(),node.line,node.column)}],
                    "explanation":if runtime.is_some(){"Source symbol with observed telemetry overlay."}else{"Source symbol discovered from AST analysis."}
                }
            })
        })
        .collect::<Vec<_>>();

    let links = graph
        .edges()
        .into_iter()
        .map(|edge| {
            let key = format!("{}->{}", edge.from.0, edge.to.0);
            let runtime = runtime_edges.get(&key);
            json!({
                "id":edge.id,
                "source":edge.from.0,
                "target":edge.to.0,
                "kind":kind(edge.kind),
                "weight":edge.weight,
                "metadata":edge.metadata,
                "runtime":runtime.map(|item| json!({
                    "invocationCount":item.invocation_count,
                    "avgLatencyMs":item.avg_latency_ms(),
                    "errorRate":item.error_rate(),
                    "lastSeenUnixNano":item.last_seen_unix_nano,
                    "traceId":item.trace_id
                })),
                "intelligence":{
                    "kind":if runtime.is_some(){"runtime"}else{"static"},
                    "confidence":1.0,
                    "evidence":[{"source":if runtime.is_some(){"otlp+ast"}else{"ast-graph"},"ref":key}]
                }
            })
        })
        .collect::<Vec<_>>();

    json!({"nodes":nodes,"links":links})
}

async fn graph_api(
    State(state): State<AppState>,
    Query(query): Query<HashMap<String, String>>,
) -> Result<Json<Value>, (StatusCode, String)> {
    let key = project_key(
        query.get("repo").map(String::as_str),
        query.get("project_id").map(String::as_str),
    );
    let session = load_session(&state, &key).await;
    let runtime_nodes = session.runtime_nodes.read().await.clone();
    let runtime_edges = session.runtime_edges.read().await.clone();
    let snapshot_id = {
        let report = session.report.read().await;
        report
            .as_ref()
            .map(|value| value.snapshot_id.clone())
            .unwrap_or_default()
    };
    let graph_guard = session.graph.read().await;
    let graph = graph_guard.as_ref().ok_or((
        StatusCode::PRECONDITION_REQUIRED,
        "No scan has been run for this project".into(),
    ))?;
    let graph = graph_json(graph, &runtime_nodes, &runtime_edges);

    Ok(Json(json!({
        "graph":graph,
        "snapshotId":snapshot_id,
        "projectKey":session.project_key,
        "generatedAt":chrono::Utc::now().to_rfc3339(),
        "persistence":"durable",
        "synthetic":false
    })))
}

fn scalar(value: &Value) -> Option<String> {
    value
        .as_str()
        .map(str::to_string)
        .or_else(|| {
            value
                .get("stringValue")
                .and_then(Value::as_str)
                .map(str::to_string)
        })
        .or_else(|| {
            value.get("intValue").and_then(|item| {
                item.as_str()
                    .map(str::to_string)
                    .or_else(|| item.as_u64().map(|number| number.to_string()))
            })
        })
}

fn attributes(value: Option<&Value>) -> HashMap<String, String> {
    let mut output = HashMap::new();
    if let Some(array) = value.and_then(Value::as_array) {
        for item in array {
            if let (Some(key), Some(value)) = (
                item.get("key").and_then(Value::as_str),
                item.get("value").and_then(scalar),
            ) {
                output.insert(key.to_string(), value);
            }
        }
    } else if let Some(object) = value.and_then(Value::as_object) {
        for (key, value) in object {
            if let Some(value) = scalar(value) {
                output.insert(key.clone(), value);
            }
        }
    }
    output
}

fn u64_value(value: Option<&Value>) -> u64 {
    value
        .and_then(Value::as_u64)
        .or_else(|| value.and_then(Value::as_str).and_then(|value| value.parse().ok()))
        .unwrap_or(0)
}

fn otlp_spans(root: &Value) -> Vec<Value> {
    if let Some(array) = root.as_array() {
        return array.clone();
    }
    let mut output = Vec::new();
    if let Some(resources) = root.get("resourceSpans").and_then(Value::as_array) {
        for resource in resources {
            let resource_attributes = attributes(
                resource
                    .get("resource")
                    .and_then(|value| value.get("attributes")),
            );
            let scopes = resource
                .get("scopeSpans")
                .or_else(|| resource.get("instrumentationLibrarySpans"))
                .and_then(Value::as_array);
            if let Some(scopes) = scopes {
                for scope in scopes {
                    if let Some(spans) = scope.get("spans").and_then(Value::as_array) {
                        for span in spans {
                            let mut merged = span.clone();
                            if let Some(object) = merged.as_object_mut() {
                                let mut merged_attributes = resource_attributes.clone();
                                merged_attributes.extend(attributes(span.get("attributes")));
                                object.insert(
                                    "_ckbMergedAttributes".into(),
                                    serde_json::to_value(merged_attributes).unwrap_or(Value::Null),
                                );
                            }
                            output.push(merged);
                        }
                    }
                }
            }
        }
    }
    output
}

fn canonical_span_id(span: &Value) -> String {
    let attrs: HashMap<String, String> = span
        .get("_ckbMergedAttributes")
        .and_then(|value| serde_json::from_value(value.clone()).ok())
        .unwrap_or_else(|| attributes(span.get("attributes")));
    let file = attrs
        .get("code.file.path")
        .or_else(|| attrs.get("code.filepath"))
        .or_else(|| attrs.get("code.file.name"));
    let function = attrs
        .get("code.function.name")
        .or_else(|| attrs.get("code.function"))
        .or_else(|| attrs.get("function.name"));
    let namespace = attrs
        .get("code.namespace")
        .or_else(|| attrs.get("service.name"));

    match (file, function, namespace) {
        (Some(file), Some(function), _) => {
            format!("{}::{}", file.replace('\\', "/"), function)
        }
        (Some(file), None, _) => format!("{}::file", file.replace('\\', "/")),
        (None, Some(function), Some(namespace)) => format!("{}::{}", namespace, function),
        (None, Some(function), None) => function.clone(),
        (None, None, Some(namespace)) => format!(
            "{}::{}",
            namespace,
            span.get("name").and_then(Value::as_str).unwrap_or("span")
        ),
        _ => span
            .get("name")
            .and_then(Value::as_str)
            .unwrap_or("span")
            .to_string(),
    }
}

fn span_is_error(span: &Value) -> bool {
    let code = span.get("status").and_then(|value| value.get("code"));
    code.and_then(Value::as_u64)
        .map(|value| value == 2)
        .or_else(|| {
            code.and_then(Value::as_str).map(|value| {
                matches!(
                    value.to_ascii_uppercase().as_str(),
                    "2" | "ERROR" | "STATUS_CODE_ERROR"
                )
            })
        })
        .unwrap_or(false)
}

fn edge_observations(raw: &str) -> anyhow::Result<HashMap<String, RuntimeEdgeObservation>> {
    let root: Value = serde_json::from_str(raw)?;
    let spans = otlp_spans(&root);
    let mut by_span_id = HashMap::new();
    for span in &spans {
        let span_id = span
            .get("spanId")
            .or_else(|| span.get("span_id"))
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();
        let trace_id = span
            .get("traceId")
            .or_else(|| span.get("trace_id"))
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();
        if !span_id.is_empty() {
            by_span_id.insert(span_id, (trace_id, canonical_span_id(span)));
        }
    }

    let mut output: HashMap<String, RuntimeEdgeObservation> = HashMap::new();
    for span in &spans {
        let parent = span
            .get("parentSpanId")
            .or_else(|| span.get("parent_span_id"))
            .and_then(Value::as_str)
            .unwrap_or("");
        let Some((parent_trace, source)) = by_span_id.get(parent).cloned() else {
            continue;
        };
        let target = canonical_span_id(span);
        let trace_id = span
            .get("traceId")
            .or_else(|| span.get("trace_id"))
            .and_then(Value::as_str)
            .unwrap_or(&parent_trace)
            .to_string();
        let start = u64_value(
            span.get("startTimeUnixNano")
                .or_else(|| span.get("start_time_unix_nano")),
        );
        let end = u64_value(
            span.get("endTimeUnixNano")
                .or_else(|| span.get("end_time_unix_nano")),
        );
        let key = format!("{}->{}", source, target);
        let entry = output.entry(key).or_insert(RuntimeEdgeObservation {
            source,
            target,
            trace_id,
            invocation_count: 0,
            error_count: 0,
            total_latency_ms: 0.0,
            last_seen_unix_nano: 0,
        });
        entry.invocation_count += 1;
        entry.total_latency_ms += end.saturating_sub(start) as f64 / 1_000_000.0;
        entry.last_seen_unix_nano = entry.last_seen_unix_nano.max(end);
        if span_is_error(span) {
            entry.error_count += 1;
        }
    }
    Ok(output)
}

fn merge_runtime_nodes(
    target: &mut HashMap<NodeId, RuntimeMetrics>,
    incoming: HashMap<NodeId, RuntimeMetrics>,
) {
    for (id, metrics) in incoming {
        let entry = target.entry(id).or_insert(RuntimeMetrics {
            execution_count: 0,
            avg_latency_ms: 0.0,
            error_rate: 0.0,
            is_hotpath: false,
        });
        let old_count = entry.execution_count;
        let total = old_count.saturating_add(metrics.execution_count);
        if total > 0 {
            entry.avg_latency_ms = ((entry.avg_latency_ms as f64 * old_count as f64
                + metrics.avg_latency_ms as f64 * metrics.execution_count as f64)
                / total as f64) as f32;
            entry.error_rate = ((entry.error_rate as f64 * old_count as f64
                + metrics.error_rate as f64 * metrics.execution_count as f64)
                / total as f64) as f32;
        }
        entry.execution_count = total;
        entry.is_hotpath = entry.is_hotpath || metrics.is_hotpath || total > 500;
    }
}

fn merge_runtime_edges(
    target: &mut HashMap<String, RuntimeEdgeObservation>,
    incoming: HashMap<String, RuntimeEdgeObservation>,
) {
    for (key, metrics) in incoming {
        if let Some(entry) = target.get_mut(&key) {
            entry.invocation_count += metrics.invocation_count;
            entry.error_count += metrics.error_count;
            entry.total_latency_ms += metrics.total_latency_ms;
            entry.last_seen_unix_nano = entry.last_seen_unix_nano.max(metrics.last_seen_unix_nano);
            entry.trace_id = metrics.trace_id;
        } else {
            target.insert(key, metrics);
        }
    }
}

async fn otlp(
    State(state): State<AppState>,
    Json(request): Json<OtlpRequest>,
) -> Result<Json<Value>, (StatusCode, String)> {
    let key = project_key(request.repo_name.as_deref(), request.project_id.as_deref());
    let session = load_session(&state, &key).await;
    let raw = if let Some(value) = request.raw_spans.or(request.otlp_json) {
        value
    } else if let Some(value) = request.payload {
        serde_json::to_string(&value).map_err(internal)?
    } else {
        return Err((
            StatusCode::BAD_REQUEST,
            "Provide raw_spans, otlp_json, or payload".into(),
        ));
    };

    let node_updates = OtlpReceiver::ingest_spans(&raw).map_err(internal)?;
    let edge_updates = edge_observations(&raw).map_err(internal)?;
    let summary = OtlpReceiver::summarize(&node_updates);
    {
        let mut nodes = session.runtime_nodes.write().await;
        merge_runtime_nodes(&mut nodes, node_updates);
    }
    {
        let mut edges = session.runtime_edges.write().await;
        merge_runtime_edges(&mut edges, edge_updates);
    }
    persist_session(&state, &session).await.map_err(internal)?;
    let runtime_edge_count = session.runtime_edges.read().await.len();

    Ok(Json(json!({
        "status":"observed",
        "kind":"runtime",
        "spansIngested":summary.spans_ingested,
        "nodesUpdated":summary.nodes_updated,
        "errorSpans":summary.error_spans,
        "hotpathNodes":summary.hotpath_nodes,
        "runtimeEdges":runtime_edge_count,
        "persistence":"durable",
        "evidence":[{"source":"otlp","ref":"ingested-payload"}],
        "synthetic":false
    })))
}

async fn runtime(
    State(state): State<AppState>,
    Query(query): Query<HashMap<String, String>>,
) -> Json<Value> {
    let key = project_key(
        query.get("repo").map(String::as_str),
        query.get("project_id").map(String::as_str),
    );
    let session = load_session(&state, &key).await;
    let nodes_guard = session.runtime_nodes.read().await;
    let nodes = nodes_guard
        .iter()
        .map(|(id, metrics)| {
            json!({
                "id":id.0,
                "invocationCount":metrics.execution_count,
                "avgLatencyMs":metrics.avg_latency_ms,
                "errorRate":metrics.error_rate,
                "isHotpath":metrics.is_hotpath,
                "kind":"runtime",
                "evidence":[{"source":"otlp","ref":id.0}]
            })
        })
        .collect::<Vec<_>>();
    drop(nodes_guard);

    let edges_guard = session.runtime_edges.read().await;
    let edges = edges_guard
        .values()
        .map(|item| {
            json!({
                "source":item.source,
                "target":item.target,
                "traceId":item.trace_id,
                "invocationCount":item.invocation_count,
                "avgLatencyMs":item.avg_latency_ms(),
                "errorRate":item.error_rate(),
                "lastSeenUnixNano":item.last_seen_unix_nano,
                "kind":"runtime",
                "evidence":[{"source":"otlp-parent-child","ref":format!("{}->{}",item.source,item.target)}]
            })
        })
        .collect::<Vec<_>>();
    drop(edges_guard);

    Json(json!({
        "observed":!nodes.is_empty() || !edges.is_empty(),
        "nodes":nodes,
        "edges":edges,
        "persistence":"durable",
        "synthetic":false
    }))
}

async fn traces(
    State(state): State<AppState>,
    Query(query): Query<HashMap<String, String>>,
) -> Json<Value> {
    let key = project_key(
        query.get("repo").map(String::as_str),
        query.get("project_id").map(String::as_str),
    );
    let session = load_session(&state, &key).await;
    let edges_guard = session.runtime_edges.read().await;
    let mut traces: HashMap<String, Vec<Value>> = HashMap::new();
    for item in edges_guard.values() {
        traces.entry(item.trace_id.clone()).or_default().push(json!({
            "source":item.source,
            "target":item.target,
            "invocationCount":item.invocation_count,
            "avgLatencyMs":item.avg_latency_ms(),
            "errorRate":item.error_rate(),
            "lastSeenUnixNano":item.last_seen_unix_nano
        }));
    }
    let observed = !edges_guard.is_empty();
    drop(edges_guard);
    Json(json!({
        "kind":"runtime",
        "observed":observed,
        "traces":traces,
        "synthetic":false
    }))
}

async fn source(
    State(state): State<AppState>,
    Query(query): Query<HashMap<String, String>>,
) -> Result<Json<Value>, (StatusCode, String)> {
    let key = project_key(
        query.get("repo").map(String::as_str),
        query.get("project_id").map(String::as_str),
    );
    let node_id = query.get("node_id").ok_or((
        StatusCode::BAD_REQUEST,
        "node_id is required".into(),
    ))?;
    let session = load_session(&state, &key).await;
    let graph_guard = session.graph.read().await;
    let graph = graph_guard
        .as_ref()
        .ok_or((StatusCode::PRECONDITION_REQUIRED, "No scan".into()))?;
    let node = graph
        .nodes()
        .into_iter()
        .find(|node| node.id.0 == *node_id)
        .ok_or((StatusCode::NOT_FOUND, "Node not found".into()))?;

    Ok(Json(json!({
        "id":node.id.0,
        "name":node.name,
        "kind":kind(node.kind),
        "path":node.path,
        "line":node.line,
        "column":node.column,
        "span":{
            "startLine":node.metadata.get("start_line"),
            "startColumn":node.metadata.get("start_column"),
            "endLine":node.metadata.get("end_line"),
            "endColumn":node.metadata.get("end_column"),
            "byteStart":node.metadata.get("byte_start"),
            "byteEnd":node.metadata.get("byte_end")
        },
        "kindOfEvidence":"static",
        "confidence":1.0,
        "evidence":[{"source":"tree-sitter-ast","ref":node.id.0}],
        "synthetic":false
    })))
}

async fn github_history(state: &AppState, repo_url: &str, max: usize) -> anyhow::Result<Value> {
    let (owner, repo) = parse_github_url(repo_url)
        .ok_or_else(|| anyhow::anyhow!("Invalid persisted GitHub URL"))?;
    let url = format!(
        "https://api.github.com/repos/{owner}/{repo}/commits?per_page={}",
        max.min(100)
    );
    let response = state
        .http
        .get(url)
        .header("Accept", "application/vnd.github+json")
        .header("User-Agent", "CKB-Software-Reality-v5")
        .header("X-GitHub-Api-Version", "2022-11-28")
        .send()
        .await?;
    if !response.status().is_success() {
        anyhow::bail!("GitHub history unavailable without repository access");
    }
    let values: Vec<Value> = response.json().await?;
    let entries = values
        .into_iter()
        .map(|value| {
            json!({
                "commit_hash":value.get("sha").and_then(Value::as_str).unwrap_or(""),
                "author":value.pointer("/commit/author/name").and_then(Value::as_str).unwrap_or(""),
                "date":value.pointer("/commit/author/date").and_then(Value::as_str).unwrap_or(""),
                "message":value.pointer("/commit/message").and_then(Value::as_str).unwrap_or(""),
                "files_changed":[],
                "additions":0,
                "deletions":0,
                "estimated_violations_introduced":0,
                "risk_score":0.0
            })
        })
        .collect::<Vec<_>>();
    let count = entries.len();
    Ok(json!({
        "entries":entries,
        "commits_analyzed":count,
        "source":"github-public-commit-api"
    }))
}

async fn history(
    State(state): State<AppState>,
    Query(query): Query<HashMap<String, String>>,
) -> Result<Json<Value>, (StatusCode, String)> {
    let key = project_key(
        query.get("repo").map(String::as_str),
        query.get("project_id").map(String::as_str),
    );
    let session = load_session(&state, &key).await;
    let max = query
        .get("max_commits")
        .and_then(|value| value.parse::<usize>().ok())
        .unwrap_or(50)
        .min(500);
    let local_path = session
        .repo_path
        .read()
        .await
        .clone()
        .or_else(|| query.get("path").cloned());
    if let Some(path) = local_path {
        let timeline = GitDriftAnalyzer::build_timeline(&path, max).map_err(internal)?;
        return Ok(Json(json!({
            "kind":"static",
            "source":"git",
            "timeline":timeline,
            "evidence":[{"source":"git-history","ref":path}],
            "synthetic":false
        })));
    }
    let repo_url = session.repo_url.read().await.clone();
    if let Some(url) = repo_url {
        let timeline = github_history(&state, &url, max).await.map_err(internal)?;
        return Ok(Json(json!({
            "kind":"static",
            "source":"github-api",
            "timeline":timeline,
            "evidence":[{"source":"github-commit-history","ref":url}],
            "synthetic":false
        })));
    }
    Err((
        StatusCode::PRECONDITION_REQUIRED,
        "No repository history source is attached to this project".into(),
    ))
}

async fn snapshots(
    State(state): State<AppState>,
    Query(query): Query<HashMap<String, String>>,
) -> Result<Json<Value>, (StatusCode, String)> {
    let key = project_key(
        query.get("repo").map(String::as_str),
        query.get("project_id").map(String::as_str),
    );
    let directory = snapshot_dir(&state, &key);
    let mut output = Vec::new();
    if let Ok(mut entries) = tokio::fs::read_dir(directory).await {
        while let Ok(Some(entry)) = entries.next_entry().await {
            if let Ok(bytes) = tokio::fs::read(entry.path()).await {
                if let Ok(snapshot) = bincode::deserialize::<ArchitectureSnapshot>(&bytes) {
                    output.push(json!({
                        "id":snapshot.id,
                        "createdAt":snapshot.created_at,
                        "nodes":snapshot.report.nodes,
                        "edges":snapshot.report.edges,
                        "filesProcessed":snapshot.report.files_processed,
                        "violations":snapshot.report.drift.len(),
                        "durationMs":snapshot.report.duration_ms
                    }));
                }
            }
        }
    }
    output.sort_by(|a, b| {
        a.get("createdAt")
            .and_then(Value::as_str)
            .cmp(&b.get("createdAt").and_then(Value::as_str))
    });
    Ok(Json(json!({
        "projectKey":key,
        "snapshots":output,
        "source":"ckb-persistent-snapshots",
        "synthetic":false
    })))
}

async fn snapshot_graph(
    State(state): State<AppState>,
    Query(query): Query<HashMap<String, String>>,
) -> Result<Json<Value>, (StatusCode, String)> {
    let key = project_key(
        query.get("repo").map(String::as_str),
        query.get("project_id").map(String::as_str),
    );
    let id = query.get("snapshot_id").ok_or((
        StatusCode::BAD_REQUEST,
        "snapshot_id is required".into(),
    ))?;
    let snapshot = read_snapshot(&state, &key, id).await.map_err(internal)?;
    let graph = graph_json(&snapshot.graph, &HashMap::new(), &HashMap::new());
    Ok(Json(json!({
        "kind":"static",
        "source":"persistent-snapshot",
        "snapshotId":snapshot.id,
        "createdAt":snapshot.created_at,
        "graph":graph,
        "report":snapshot.report,
        "synthetic":false
    })))
}

/// The node-identity normalization the snapshot diff applies to BOTH
/// sides before comparing. Snapshots persisted before the
/// project-relative fix carry this server's extraction scaffolding in
/// every node ID (`/tmp/ckb-zip-<uuid>/src/…`,
/// `/tmp/ckb-reality-<uuid>/<zipball wrapper>/src/…`); stored files are
/// never rewritten, so old snapshots are identified by stripping
/// everything up to and including the last extraction-directory
/// segment. New snapshots are already project-relative and pass
/// through unchanged.
fn diff_identity(id: &str) -> String {
    let (path, symbol) = match id.split_once("::") {
        Some((path, symbol)) => (path, Some(symbol)),
        None => (id, None),
    };
    let segments: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();
    let cut = segments
        .iter()
        .rposition(|s| s.starts_with("ckb-zip-") || s.starts_with("ckb-reality-"))
        .map(|index| index + 1)
        .unwrap_or(0);
    let relative = segments[cut..].join("/");
    match symbol {
        Some(symbol) => format!("{relative}::{symbol}"),
        None => relative,
    }
}

/// Split a node ID into path segments and symbol suffix.
fn split_node_id(id: &str) -> (Vec<&str>, &str) {
    let (path, symbol) = id.split_once("::").unwrap_or((id, ""));
    (
        path.split('/').filter(|s| !s.is_empty()).collect(),
        symbol,
    )
}

/// Segment overlap when two IDs denote the same project node under
/// different scan roots: same symbol, and the shorter path is a
/// whole-segment suffix of the longer (the difference is exactly the
/// root/wrapper prefix one scan had and the other did not). Returns
/// the overlap length so callers can prefer the most specific match.
fn suffix_overlap(a: &str, b: &str) -> Option<usize> {
    let (path_a, symbol_a) = split_node_id(a);
    let (path_b, symbol_b) = split_node_id(b);
    if symbol_a != symbol_b || path_a.is_empty() || path_b.is_empty() {
        return None;
    }
    let (short, long) = if path_a.len() <= path_b.len() {
        (path_a, path_b)
    } else {
        (path_b, path_a)
    };
    if long[long.len() - short.len()..] == short[..] {
        Some(short.len())
    } else {
        None
    }
}

/// Map both sides' (already `diff_identity`-normalized) node IDs to a
/// shared canonical identity: exact matches map to themselves; IDs
/// that only differ by a root/wrapper prefix pair up by longest
/// segment-suffix overlap and map to the shorter — project-relative —
/// form. Unpaired IDs map to themselves, so a genuinely added or
/// removed node keeps its own identity in the diff output.
fn canonical_id_map(
    from_ids: &HashSet<String>,
    to_ids: &HashSet<String>,
) -> HashMap<String, String> {
    let mut map: HashMap<String, String> = HashMap::new();
    let mut paired_to: HashSet<&str> = HashSet::new();

    // Most specific (longest-path) IDs claim their partners first.
    let mut from_sorted: Vec<&String> = from_ids.iter().collect();
    from_sorted.sort_by(|a, b| {
        split_node_id(b)
            .0
            .len()
            .cmp(&split_node_id(a).0.len())
            .then_with(|| a.cmp(b))
    });

    for from_id in from_sorted {
        if to_ids.contains(from_id) {
            map.insert(from_id.clone(), from_id.clone());
            paired_to.insert(from_id.as_str());
            continue;
        }
        let best = to_ids
            .iter()
            .filter(|candidate| !paired_to.contains(candidate.as_str()))
            .filter_map(|candidate| {
                suffix_overlap(from_id, candidate).map(|overlap| (overlap, candidate))
            })
            .max_by(|a, b| a.0.cmp(&b.0).then_with(|| b.1.cmp(a.1)));
        if let Some((_, partner)) = best {
            let canonical = if split_node_id(from_id).0.len() <= split_node_id(partner).0.len() {
                from_id.clone()
            } else {
                partner.clone()
            };
            map.insert(from_id.clone(), canonical.clone());
            map.insert(partner.clone(), canonical);
            paired_to.insert(partner.as_str());
        }
    }
    for to_id in to_ids {
        map.entry(to_id.clone()).or_insert_with(|| to_id.clone());
    }
    map
}

struct SnapshotDiff {
    added_nodes: Vec<String>,
    removed_nodes: Vec<String>,
    added_edges: Vec<String>,
    removed_edges: Vec<String>,
    node_delta: i64,
    edge_delta: i64,
    from_violations: usize,
    to_violations: usize,
}

/// Structural diff between two persisted snapshots. Identity is
/// matched on project-relative node IDs: new snapshots store them
/// directly; legacy snapshots are normalized at compare time (see
/// `diff_identity` / `canonical_id_map`), so a legacy snapshot and a
/// new snapshot of the same tree diff to zero — stored files are
/// never migrated or rewritten.
fn compute_snapshot_diff(from: &ArchitectureSnapshot, to: &ArchitectureSnapshot) -> SnapshotDiff {
    let from_raw: HashSet<String> = from
        .graph
        .nodes()
        .into_iter()
        .map(|node| diff_identity(&node.id.0))
        .collect();
    let to_raw: HashSet<String> = to
        .graph
        .nodes()
        .into_iter()
        .map(|node| diff_identity(&node.id.0))
        .collect();
    let canonical = canonical_id_map(&from_raw, &to_raw);
    let canon = |id: &str| -> String {
        let normalized = diff_identity(id);
        canonical
            .get(&normalized)
            .cloned()
            .unwrap_or(normalized)
    };
    let from_nodes: HashSet<String> = from_raw.iter().map(|id| canon(id)).collect();
    let to_nodes: HashSet<String> = to_raw.iter().map(|id| canon(id)).collect();

    let edge_key = |edge: &ckb_core::Edge| -> String {
        format!(
            "{}->{}/{:?}",
            canon(&edge.from.0),
            canon(&edge.to.0),
            edge.kind
        )
        .to_ascii_lowercase()
    };
    let from_edges: HashSet<String> = from.graph.edges().into_iter().map(edge_key).collect();
    let to_edges: HashSet<String> = to.graph.edges().into_iter().map(edge_key).collect();

    let mut added_nodes = to_nodes.difference(&from_nodes).cloned().collect::<Vec<_>>();
    let mut removed_nodes = from_nodes.difference(&to_nodes).cloned().collect::<Vec<_>>();
    let mut added_edges = to_edges.difference(&from_edges).cloned().collect::<Vec<_>>();
    let mut removed_edges = from_edges.difference(&to_edges).cloned().collect::<Vec<_>>();
    added_nodes.sort();
    removed_nodes.sort();
    added_edges.sort();
    removed_edges.sort();

    SnapshotDiff {
        node_delta: to_nodes.len() as i64 - from_nodes.len() as i64,
        edge_delta: to_edges.len() as i64 - from_edges.len() as i64,
        added_nodes,
        removed_nodes,
        added_edges,
        removed_edges,
        from_violations: from.report.drift.len(),
        to_violations: to.report.drift.len(),
    }
}

async fn diff(
    State(state): State<AppState>,
    Json(request): Json<DiffRequest>,
) -> Result<Json<Value>, (StatusCode, String)> {
    let key = project_key(request.repo_name.as_deref(), request.project_id.as_deref());
    let from = read_snapshot(&state, &key, &request.from_snapshot)
        .await
        .map_err(internal)?;
    let to = read_snapshot(&state, &key, &request.to_snapshot)
        .await
        .map_err(internal)?;

    let delta = compute_snapshot_diff(&from, &to);

    Ok(Json(json!({
        "kind":"static",
        "source":"persistent-snapshot-diff",
        "from":request.from_snapshot,
        "to":request.to_snapshot,
        "addedNodes":delta.added_nodes,
        "removedNodes":delta.removed_nodes,
        "addedEdges":delta.added_edges,
        "removedEdges":delta.removed_edges,
        "summary":{
            "nodeDelta":delta.node_delta,
            "edgeDelta":delta.edge_delta,
            "fromViolations":delta.from_violations,
            "toViolations":delta.to_violations
        },
        "evidence":[
            {"source":"ckb-snapshot","ref":from.id},
            {"source":"ckb-snapshot","ref":to.id}
        ],
        "synthetic":false
    })))
}

async fn causal_path(
    State(state): State<AppState>,
    Json(request): Json<CausalPathRequest>,
) -> Result<Json<Value>, (StatusCode, String)> {
    let key = project_key(request.repo_name.as_deref(), request.project_id.as_deref());
    let session = load_session(&state, &key).await;
    let graph_guard = session.graph.read().await;
    let graph = graph_guard
        .as_ref()
        .ok_or((StatusCode::PRECONDITION_REQUIRED, "No scan".into()))?;
    let report = CausalArchitectureEngine::shortest_path(
        graph,
        &NodeId(request.source),
        &NodeId(request.target),
        request.max_depth.unwrap_or(12),
    )
    .map_err(internal)?;
    Ok(Json(serde_json::to_value(report).map_err(internal)?))
}

async fn failure_cone(
    State(state): State<AppState>,
    Json(request): Json<FailureConeRequest>,
) -> Result<Json<Value>, (StatusCode, String)> {
    let key = project_key(request.repo_name.as_deref(), request.project_id.as_deref());
    let session = load_session(&state, &key).await;
    let graph_guard = session.graph.read().await;
    let graph = graph_guard
        .as_ref()
        .ok_or((StatusCode::PRECONDITION_REQUIRED, "No scan".into()))?;
    let report = CausalArchitectureEngine::failure_cone(
        graph,
        &NodeId(request.root),
        request.max_depth.unwrap_or(12),
    )
    .map_err(internal)?;
    Ok(Json(serde_json::to_value(report).map_err(internal)?))
}

async fn memory_query(
    State(state): State<AppState>,
    Json(request): Json<MemoryQueryRequest>,
) -> Result<Json<Value>, (StatusCode, String)> {
    let key = project_key(request.repo_name.as_deref(), request.project_id.as_deref());
    let session = load_session(&state, &key).await;
    let graph_guard = session.graph.read().await;
    let graph = graph_guard
        .as_ref()
        .ok_or((StatusCode::PRECONDITION_REQUIRED, "No scan".into()))?;
    let report = ArchitectureMemoryEngine::query(
        graph,
        &request.query,
        request.depth.unwrap_or(2),
        request.limit.unwrap_or(12),
    )
    .map_err(internal)?;
    Ok(Json(serde_json::to_value(report).map_err(internal)?))
}

async fn code_dna(
    State(state): State<AppState>,
    Query(query): Query<HashMap<String, String>>,
) -> Result<Json<Value>, (StatusCode, String)> {
    let key = project_key(
        query.get("repo").map(String::as_str),
        query.get("project_id").map(String::as_str),
    );
    let session = load_session(&state, &key).await;
    let graph_guard = session.graph.read().await;
    let graph = graph_guard
        .as_ref()
        .ok_or((StatusCode::PRECONDITION_REQUIRED, "No scan".into()))?;
    let report = ArchitectureMemoryEngine::code_dna(graph).map_err(internal)?;
    Ok(Json(serde_json::to_value(report).map_err(internal)?))
}

async fn test_gaps(
    State(state): State<AppState>,
    Query(query): Query<HashMap<String, String>>,
) -> Result<Json<Value>, (StatusCode, String)> {
    let key = project_key(
        query.get("repo").map(String::as_str),
        query.get("project_id").map(String::as_str),
    );
    let session = load_session(&state, &key).await;
    let graph_guard = session.graph.read().await;
    let graph = graph_guard.as_ref().ok_or((
        StatusCode::PRECONDITION_REQUIRED,
        "No scan".into(),
    ))?;
    let report = TestCoverageAnalyzer::analyze_gaps(graph).map_err(internal)?;
    Ok(Json(serde_json::to_value(report).map_err(internal)?))
}

async fn rules(
    State(state): State<AppState>,
    Query(query): Query<HashMap<String, String>>,
) -> Result<String, (StatusCode, String)> {
    let key = project_key(
        query.get("repo").map(String::as_str),
        query.get("project_id").map(String::as_str),
    );
    let session = load_session(&state, &key).await;
    let graph_guard = session.graph.read().await;
    let graph = graph_guard.as_ref().ok_or((
        StatusCode::PRECONDITION_REQUIRED,
        "No scan".into(),
    ))?;
    ArchitectureAnalyzer::new()
        .generate_ai_guidelines(graph)
        .map_err(internal)
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt::init();
    let data_dir = PathBuf::from(
        std::env::var("CKB_REALITY_DATA_DIR")
            .unwrap_or_else(|_| "./ckb_reality_data".into()),
    );
    tokio::fs::create_dir_all(&data_dir).await?;
    let http = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(120))
        .build()?;
    let state = AppState {
        sessions: Arc::new(RwLock::new(HashMap::new())),
        api_key: std::env::var("CKB_API_KEY")
            .ok()
            .filter(|value| !value.is_empty())
            .map(Arc::new),
        data_dir: Arc::new(data_dir),
        http,
    };
    if state.api_key.is_none() {
        warn!("CKB_API_KEY is not configured; Reality API is unauthenticated");
    }

    let protected = Router::new()
        .route("/api/v1/scan", post(scan))
        .route("/api/v1/report", get(report))
        .route("/api/v1/impact", post(impact))
        .route("/api/v1/otlp", post(otlp))
        .route("/api/v1/drift-timeline", get(history))
        .route("/api/v1/test-gaps", get(test_gaps))
        .route("/api/v1/rules", get(rules))
        .route("/api/v1/intelligence/scan/github", post(scan_github))
        .route("/api/v1/intelligence/scan/zip", post(scan_zip))
        .route("/api/v1/intelligence/graph", get(graph_api))
        .route("/api/v1/intelligence/source", get(source))
        .route("/api/v1/intelligence/runtime", get(runtime))
        .route("/api/v1/intelligence/traces", get(traces))
        .route("/api/v1/intelligence/impact", post(impact))
        .route("/api/v1/intelligence/telemetry/otlp", post(otlp))
        .route("/api/v1/intelligence/history", get(history))
        .route("/api/v1/intelligence/snapshots", get(snapshots))
        .route("/api/v1/intelligence/snapshot", get(snapshot_graph))
        .route("/api/v1/intelligence/diff", post(diff))
        .route("/api/v1/intelligence/causal-path", post(causal_path))
        .route("/api/v1/intelligence/failure-cone", post(failure_cone))
        .route("/api/v1/intelligence/memory/query", post(memory_query))
        .route("/api/v1/intelligence/code-dna", get(code_dna))
        .route_layer(middleware::from_fn_with_state(state.clone(), auth));

    let cors = match std::env::var("CKB_ALLOWED_ORIGIN") {
        Ok(value) if value != "*" => CorsLayer::new()
            .allow_origin(value.parse::<axum::http::HeaderValue>()?)
            .allow_methods(Any)
            .allow_headers(Any),
        _ => CorsLayer::new()
            .allow_origin(Any)
            .allow_methods(Any)
            .allow_headers(Any),
    };

    let app = Router::new()
        .route("/health", get(health))
        .merge(protected)
        .layer(DefaultBodyLimit::max(85 * 1024 * 1024))
        .layer(cors)
        .with_state(state);

    let port = std::env::var("PORT")
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(3000);
    let bind_all = std::env::var("CKB_BIND_ALL")
        .map(|value| value == "1" || value.eq_ignore_ascii_case("true"))
        .unwrap_or(false);
    let host = if bind_all { [0, 0, 0, 0] } else { [127, 0, 0, 1] };
    let address = std::net::SocketAddr::from((host, port));
    info!("CKB Reality API v5 listening on {}", address);
    let listener = tokio::net::TcpListener::bind(address).await?;
    axum::serve(listener, app).await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture(name: &str) -> String {
        format!("{}/tests/fixtures/{}", env!("CARGO_MANIFEST_DIR"), name)
    }

    fn test_state() -> (AppState, PathBuf) {
        let data_dir =
            std::env::temp_dir().join(format!("ckb-v5-test-{}", uuid::Uuid::new_v4()));
        let state = AppState {
            sessions: Arc::new(RwLock::new(HashMap::new())),
            api_key: None,
            data_dir: Arc::new(data_dir.clone()),
            http: reqwest::Client::new(),
        };
        (state, data_dir)
    }

    fn copy_dir(source: &Path, target: &Path) {
        std::fs::create_dir_all(target).unwrap();
        for entry in std::fs::read_dir(source).unwrap() {
            let entry = entry.unwrap();
            let destination = target.join(entry.file_name());
            if entry.path().is_dir() {
                copy_dir(&entry.path(), &destination);
            } else {
                std::fs::copy(entry.path(), &destination).unwrap();
            }
        }
    }

    /// Copy a fixture into a fake per-scan extraction dir, mirroring
    /// what `scan_zip` extracts into (`temp_dir()/ckb-zip-<uuid>`).
    fn extracted_copy(fixture_name: &str) -> PathBuf {
        let temp = std::env::temp_dir().join(format!("ckb-zip-{}", uuid::Uuid::new_v4()));
        copy_dir(Path::new(&fixture(fixture_name)), &temp);
        temp
    }

    fn kind_of(violation: &ckb_core::DriftViolation) -> String {
        format!("{:?}", violation.kind)
    }

    #[tokio::test]
    async fn fixture_c_scan_enforces_project_rules() {
        let (_graph, report) = build_graph(&fixture("fixture-c")).await.unwrap();
        assert_eq!(report.files_processed, 10);
        assert_eq!(report.nodes, 24);
        assert_eq!(report.edges, 31);
        assert_eq!(
            report.drift.len(),
            2,
            "drift: {:?}",
            report.drift.iter().map(kind_of).collect::<Vec<_>>()
        );
        let kinds: Vec<String> = report.drift.iter().map(kind_of).collect();
        assert!(kinds.contains(&"CircularDependency".to_string()));
        let forbidden = report
            .drift
            .iter()
            .find(|v| kind_of(v) == "ForbiddenDependency")
            .expect("fixture-c must produce a ForbiddenDependency violation");
        assert_eq!(forbidden.boundary, "ui-must-not-import-db");
        assert_eq!(format!("{:?}", forbidden.severity), "Error");
        assert!(forbidden.from.0.ends_with("src/ui/panel.ts::file"), "{}", forbidden.from.0);
        assert!(forbidden.to.0.ends_with("src/db/store.ts::file"), "{}", forbidden.to.0);
        assert!(forbidden.message.contains("ui-must-not-import-db"));
        // The sanctioned service -> db import is scoped out by the rule's `from`.
        assert!(
            !report
                .drift
                .iter()
                .any(|v| v.from.0.ends_with("src/service/api.ts::file")),
            "service->db must not be flagged"
        );
    }

    #[tokio::test]
    async fn fixture_a_scan_has_exactly_the_cycle() {
        let (_graph, report) = build_graph(&fixture("fixture-a")).await.unwrap();
        assert_eq!(report.files_processed, 10);
        assert_eq!(report.nodes, 24);
        assert_eq!(report.edges, 31);
        assert_eq!(report.drift.len(), 1);
        assert_eq!(kind_of(&report.drift[0]), "CircularDependency");
    }

    #[tokio::test]
    async fn fixture_b_scan_is_clean() {
        let (_graph, report) = build_graph(&fixture("fixture-b")).await.unwrap();
        assert_eq!(report.files_processed, 11);
        assert_eq!(report.nodes, 26);
        assert_eq!(report.edges, 33);
        assert!(report.drift.is_empty());
    }

    #[tokio::test]
    async fn malformed_rules_file_fails_the_scan_loudly() {
        let root = std::env::temp_dir().join(format!("ckb-v5-badrules-{}", uuid::Uuid::new_v4()));
        let src = root.join("src");
        std::fs::create_dir_all(root.join(".ckb")).unwrap();
        std::fs::create_dir_all(&src).unwrap();
        std::fs::write(src.join("a.ts"), "import { b } from \"./b\";\nexport const a = b;\n").unwrap();
        std::fs::write(src.join("b.ts"), "export const b = 1;\n").unwrap();
        std::fs::write(root.join(".ckb").join("rules.json"), "[{ not json").unwrap();

        let error = build_graph(&root.to_string_lossy()).await.unwrap_err();
        let text = format!("{error:?}");
        assert!(text.contains("rules.json"), "error must name the rules file: {text}");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[tokio::test]
    async fn persisted_snapshot_stores_project_relative_ids() {
        let extracted = extracted_copy("fixture-a");
        let root = extracted.to_string_lossy().to_string();
        let (graph, report) = build_graph(&root).await.unwrap();
        assert!(
            graph.nodes().iter().any(|n| n.id.0.contains("ckb-zip-")),
            "the live graph is rooted at the extraction dir"
        );

        let (state, data_dir) = test_state();
        let response = save_scan(
            &state,
            "test-project".to_string(),
            graph,
            report,
            None,
            Some("fixture-a.zip".to_string()),
            &root,
        )
        .await
        .unwrap();
        let snapshot_id = response.0["snapshotId"].as_str().unwrap().to_string();
        let snapshot = read_snapshot(&state, "test-project", &snapshot_id)
            .await
            .unwrap();

        assert_eq!(snapshot.graph.node_count(), 24);
        assert_eq!(snapshot.graph.edge_count(), 31);
        assert!(snapshot
            .graph
            .nodes()
            .iter()
            .all(|n| !n.id.0.contains("ckb-zip-") && !n.path.to_string_lossy().contains("ckb-zip-")));
        let ids: HashSet<&str> = snapshot.graph.nodes().iter().map(|n| n.id.0.as_str()).collect();
        assert!(ids.contains("src/core/util.ts::file"), "ids: {ids:?}");
        assert!(ids.contains("src/ui/panel.ts::file"), "ids: {ids:?}");
        for edge in snapshot.graph.edges() {
            assert!(
                !edge.from.0.contains("ckb-zip-") && !edge.to.0.contains("ckb-zip-"),
                "edge {} -> {}",
                edge.from.0,
                edge.to.0
            );
        }
        assert_eq!(snapshot.report.drift.len(), 1);
        assert_eq!(snapshot.report.drift[0].from.0, "src/core/a.ts::file");
        assert_eq!(snapshot.report.drift[0].to.0, "src/core/b.ts::file");

        let _ = std::fs::remove_dir_all(&extracted);
        let _ = std::fs::remove_dir_all(&data_dir);
    }

    #[tokio::test]
    async fn snapshot_diff_a_to_b_matches_ground_truth() {
        let (state, data_dir) = test_state();
        let mut snapshot_ids = Vec::new();
        for name in ["fixture-a", "fixture-b"] {
            let extracted = extracted_copy(name);
            let root = extracted.to_string_lossy().to_string();
            let (graph, report) = build_graph(&root).await.unwrap();
            let response = save_scan(
                &state,
                "proj".to_string(),
                graph,
                report,
                None,
                Some(format!("{name}.zip")),
                &root,
            )
            .await
            .unwrap();
            snapshot_ids.push(response.0["snapshotId"].as_str().unwrap().to_string());
            let _ = std::fs::remove_dir_all(&extracted);
        }
        let snap_a = read_snapshot(&state, "proj", &snapshot_ids[0]).await.unwrap();
        let snap_b = read_snapshot(&state, "proj", &snapshot_ids[1]).await.unwrap();

        let delta = compute_snapshot_diff(&snap_a, &snap_b);
        assert_eq!(delta.node_delta, 2);
        assert_eq!(delta.edge_delta, 2);
        assert_eq!(delta.from_violations, 1);
        assert_eq!(delta.to_violations, 0);
        assert_eq!(
            delta.added_nodes,
            vec![
                "src/service/report.ts::buildReport".to_string(),
                "src/service/report.ts::file".to_string()
            ]
        );
        assert!(delta.removed_nodes.is_empty(), "removed: {:?}", delta.removed_nodes);
        assert!(delta
            .removed_edges
            .contains(&"src/core/b.ts::file->src/core/a.ts::file/import".to_string()));
        assert!(delta
            .added_edges
            .contains(&"src/service/report.ts::file->src/core/util.ts::file/import".to_string()));

        let _ = std::fs::remove_dir_all(&data_dir);
    }

    #[tokio::test]
    async fn snapshot_diff_legacy_vs_new_same_tree_is_zero() {
        let extracted = extracted_copy("fixture-a");
        let root = extracted.to_string_lossy().to_string();
        let (graph, report) = build_graph(&root).await.unwrap();
        // A pre-fix persisted snapshot: identities rooted at the
        // extraction dir, exactly as the old persist path stored them.
        let legacy = ArchitectureSnapshot {
            id: "legacy".to_string(),
            project_key: "proj".to_string(),
            created_at: "2026-01-01T00:00:00Z".to_string(),
            report: report.clone(),
            graph: graph.clone(),
        };

        let (state, data_dir) = test_state();
        let response = save_scan(
            &state,
            "proj".to_string(),
            graph,
            report,
            None,
            Some("fixture-a.zip".to_string()),
            &root,
        )
        .await
        .unwrap();
        let snapshot_id = response.0["snapshotId"].as_str().unwrap().to_string();
        let new = read_snapshot(&state, "proj", &snapshot_id).await.unwrap();

        for delta in [
            compute_snapshot_diff(&legacy, &new),
            compute_snapshot_diff(&new, &legacy),
        ] {
            assert!(delta.added_nodes.is_empty(), "added: {:?}", delta.added_nodes);
            assert!(delta.removed_nodes.is_empty(), "removed: {:?}", delta.removed_nodes);
            assert!(delta.added_edges.is_empty(), "added: {:?}", delta.added_edges);
            assert!(delta.removed_edges.is_empty(), "removed: {:?}", delta.removed_edges);
            assert_eq!(delta.node_delta, 0);
            assert_eq!(delta.edge_delta, 0);
        }

        let _ = std::fs::remove_dir_all(&extracted);
        let _ = std::fs::remove_dir_all(&data_dir);
    }

    #[tokio::test]
    async fn snapshot_diff_two_legacy_extractions_of_same_tree_is_zero() {
        let mut legacy_snapshots = Vec::new();
        for _ in 0..2 {
            let extracted = extracted_copy("fixture-a");
            let (graph, report) = build_graph(&extracted.to_string_lossy())
                .await
                .unwrap();
            legacy_snapshots.push(ArchitectureSnapshot {
                id: uuid::Uuid::new_v4().to_string(),
                project_key: "proj".to_string(),
                created_at: "2026-01-01T00:00:00Z".to_string(),
                report,
                graph,
            });
            let _ = std::fs::remove_dir_all(&extracted);
        }
        let delta = compute_snapshot_diff(&legacy_snapshots[0], &legacy_snapshots[1]);
        assert!(delta.added_nodes.is_empty(), "added: {:?}", delta.added_nodes);
        assert!(delta.removed_nodes.is_empty(), "removed: {:?}", delta.removed_nodes);
        assert!(delta.added_edges.is_empty(), "added: {:?}", delta.added_edges);
        assert!(delta.removed_edges.is_empty(), "removed: {:?}", delta.removed_edges);
        assert_eq!(delta.node_delta, 0);
        assert_eq!(delta.edge_delta, 0);
    }

    #[tokio::test]
    async fn snapshot_diff_zipball_wrapper_legacy_matches_new() {
        // GitHub zipball layout: extraction root contains one wrapper
        // directory (<owner>-<repo>-<sha>) and the old persist path
        // rooted identities at it — no ckb-* marker segment at all in
        // the stored IDs beyond the temp parent.
        let wrapped_root = std::env::temp_dir()
            .join(format!("ckb-v5-wrap-{}", uuid::Uuid::new_v4()))
            .join("someowner-somerepo-deadbeefcafe");
        copy_dir(Path::new(&fixture("fixture-a")), &wrapped_root);
        let (graph, report) = build_graph(&wrapped_root.to_string_lossy())
            .await
            .unwrap();
        let legacy = ArchitectureSnapshot {
            id: "legacy".to_string(),
            project_key: "proj".to_string(),
            created_at: "2026-01-01T00:00:00Z".to_string(),
            report: report.clone(),
            graph: graph.clone(),
        };

        let (state, data_dir) = test_state();
        let response = save_scan(
            &state,
            "proj".to_string(),
            graph,
            report,
            None,
            None,
            &wrapped_root.to_string_lossy(),
        )
        .await
        .unwrap();
        let snapshot_id = response.0["snapshotId"].as_str().unwrap().to_string();
        let new = read_snapshot(&state, "proj", &snapshot_id).await.unwrap();
        assert!(new
            .graph
            .nodes()
            .iter()
            .all(|n| n.id.0.starts_with("src/")));

        let delta = compute_snapshot_diff(&legacy, &new);
        assert!(delta.added_nodes.is_empty(), "added: {:?}", delta.added_nodes);
        assert!(delta.removed_nodes.is_empty(), "removed: {:?}", delta.removed_nodes);
        assert!(delta.added_edges.is_empty(), "added: {:?}", delta.added_edges);
        assert!(delta.removed_edges.is_empty(), "removed: {:?}", delta.removed_edges);
        assert_eq!(delta.node_delta, 0);
        assert_eq!(delta.edge_delta, 0);

        let _ = std::fs::remove_dir_all(wrapped_root.parent().unwrap());
        let _ = std::fs::remove_dir_all(&data_dir);
    }

    #[test]
    fn diff_identity_strips_extraction_scaffolding() {
        assert_eq!(
            diff_identity("/tmp/ckb-zip-9f86d081/src/core/a.ts::file"),
            "src/core/a.ts::file"
        );
        assert_eq!(
            diff_identity("/tmp/ckb-reality-9f86d081/owner-repo-abc123/src/a.ts::file"),
            "owner-repo-abc123/src/a.ts::file",
            "the zipball wrapper segment survives marker stripping; suffix pairing aligns it"
        );
        assert_eq!(diff_identity("src/core/a.ts::file"), "src/core/a.ts::file");
    }

    #[test]
    fn canonical_map_prefers_the_most_specific_suffix_match() {
        let from: HashSet<String> = [
            "proj/app/src/a.ts::file".to_string(),
            "proj/src/a.ts::file".to_string(),
        ]
        .into_iter()
        .collect();
        let to: HashSet<String> = [
            "app/src/a.ts::file".to_string(),
            "src/a.ts::file".to_string(),
        ]
        .into_iter()
        .collect();
        let map = canonical_id_map(&from, &to);
        assert_eq!(map["proj/app/src/a.ts::file"], "app/src/a.ts::file");
        assert_eq!(map["proj/src/a.ts::file"], "src/a.ts::file");
        assert_eq!(map["app/src/a.ts::file"], "app/src/a.ts::file");
        assert_eq!(map["src/a.ts::file"], "src/a.ts::file");
    }
}
