use axum::{
    extract::{DefaultBodyLimit, Query, State},
    http::StatusCode,
    routing::{get, post},
    Json, Router,
};
use base64::{engine::general_purpose::STANDARD as BASE64, Engine as _};
use ckb_core::{
    ArchitectureMemoryEngine, CausalArchitectureEngine, ChangeType, CkbEngine,
    DependencyGraph, NodeId, NodeKind, ScanReport,
};
use serde::Deserialize;
use serde_json::{json, Value};
use std::{
    collections::{HashMap, HashSet},
    io::{Cursor, Write},
    net::SocketAddr,
    path::{Path, PathBuf},
    sync::Arc,
};
use tokio::sync::{RwLock, Semaphore};
use tower_http::cors::{Any, CorsLayer};
use zip::ZipArchive;

const MAX_ARCHIVE_BYTES: usize = 60 * 1024 * 1024;
const MAX_EXTRACTED_BYTES: u64 = 300 * 1024 * 1024;
const MAX_ARCHIVE_FILES: usize = 30_000;

#[derive(Clone)]
struct AppState {
    engine: Arc<CkbEngine>,
    latest_report: Arc<RwLock<Option<ScanReport>>>,
    repo_url: Arc<RwLock<Option<String>>>,
    repo_path: Arc<RwLock<Option<String>>>,
    scan_gate: Arc<Semaphore>,
    http: reqwest::Client,
    allow_local_scan: bool,
}

#[derive(Deserialize)]
struct ScanRequest {
    path: String,
    project_id: Option<String>,
    repo_name: Option<String>,
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
    file: String,
    line: Option<u32>,
    change_type: Option<String>,
    project_id: Option<String>,
    repo_name: Option<String>,
}

#[derive(Deserialize)]
struct OtlpRequest {
    raw_spans: Option<String>,
    otlp_json: Option<String>,
    payload: Option<Value>,
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
struct DiffRequest {
    from_snapshot: String,
    to_snapshot: String,
    project_id: Option<String>,
    repo_name: Option<String>,
}

fn internal<E: std::fmt::Display>(error: E) -> (StatusCode, String) {
    (StatusCode::INTERNAL_SERVER_ERROR, error.to_string())
}

fn bad<E: std::fmt::Display>(error: E) -> (StatusCode, String) {
    (StatusCode::BAD_REQUEST, error.to_string())
}

fn kind<T: std::fmt::Debug>(value: T) -> String {
    format!("{:?}", value).to_ascii_lowercase()
}

fn project_label(project_id: Option<&str>, repo_name: Option<&str>) -> String {
    project_id
        .filter(|value| !value.trim().is_empty())
        .or_else(|| repo_name.filter(|value| !value.trim().is_empty()))
        .unwrap_or("current")
        .to_string()
}

fn parse_change_type(value: Option<&str>) -> ChangeType {
    match value.unwrap_or("modify").to_ascii_lowercase().as_str() {
        "add" => ChangeType::Add,
        "delete" => ChangeType::Delete,
        "rename" => ChangeType::Rename,
        _ => ChangeType::Modify,
    }
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

    let mut total_size = 0u64;
    for index in 0..zip.len() {
        let mut entry = zip.by_index(index)?;
        total_size = total_size.saturating_add(entry.size());
        if total_size > MAX_EXTRACTED_BYTES {
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

fn runtime_json(metrics: &ckb_core::RuntimeMetrics) -> Value {
    json!({
        "invocationCount": metrics.execution_count,
        "avgLatencyMs": metrics.avg_latency_ms,
        "errorRate": metrics.error_rate,
        "isHotpath": metrics.is_hotpath
    })
}

fn graph_value(graph: &DependencyGraph) -> Value {
    let nodes = graph
        .nodes()
        .into_iter()
        .map(|node| {
            let runtime = graph.get_runtime_metrics(&node.id);
            json!({
                "id": node.id.0,
                "name": node.name,
                "kind": kind(node.kind),
                "path": node.path,
                "line": node.line,
                "column": node.column,
                "metadata": node.metadata,
                "span": {
                    "startLine": node.metadata.get("start_line"),
                    "startColumn": node.metadata.get("start_column"),
                    "endLine": node.metadata.get("end_line"),
                    "endColumn": node.metadata.get("end_column"),
                    "byteStart": node.metadata.get("byte_start"),
                    "byteEnd": node.metadata.get("byte_end")
                },
                "runtime": runtime.map(runtime_json),
                "intelligence": {
                    "kind": if runtime.is_some() { "runtime" } else { "static" },
                    "confidence": 1.0,
                    "evidence": [{
                        "source": "tree-sitter-ast",
                        "ref": format!("{}:{}:{}", node.path.to_string_lossy(), node.line, node.column)
                    }]
                }
            })
        })
        .collect::<Vec<_>>();

    let links = graph
        .edges()
        .into_iter()
        .map(|edge| {
            json!({
                "id": edge.id,
                "source": edge.from.0,
                "target": edge.to.0,
                "kind": kind(edge.kind),
                "weight": edge.weight,
                "metadata": edge.metadata,
                "intelligence": {
                    "kind": "static",
                    "confidence": 1.0,
                    "evidence": [{
                        "source": "ckb-graph",
                        "ref": format!("{}->{}", edge.from.0, edge.to.0)
                    }]
                }
            })
        })
        .collect::<Vec<_>>();

    json!({ "nodes": nodes, "links": links })
}

fn resolve_node_id(graph: &DependencyGraph, requested: &str) -> Option<NodeId> {
    let requested = requested.replace('\\', "/");
    let mut matches = Vec::new();

    for node in graph.nodes() {
        let raw_id = node.id.0.replace('\\', "/");
        let path = node.path.to_string_lossy().replace('\\', "/");
        let exact = raw_id == requested;
        let id_suffix = raw_id.ends_with(&format!("/{requested}"));
        let file_match = node.kind == NodeKind::File
            && (path == requested || path.ends_with(&format!("/{requested}")));
        if exact || id_suffix || file_match {
            matches.push(node.id.clone());
        }
    }

    matches.sort_by(|a, b| a.0.cmp(&b.0));
    matches.dedup();
    if matches.len() == 1 {
        matches.into_iter().next()
    } else {
        None
    }
}

async fn current_snapshot_id(state: &AppState) -> Option<String> {
    if let Some(report) = state.latest_report.read().await.as_ref() {
        return Some(report.snapshot_id.clone());
    }
    let mut snapshots = state.engine.architecture_snapshot_metadata().await.ok()?;
    snapshots.sort_by(|a, b| a.timestamp.cmp(&b.timestamp));
    snapshots.last().map(|snapshot| snapshot.id.clone())
}

async fn remember_scan(
    state: &AppState,
    report: ScanReport,
    repo_url: Option<String>,
    repo_path: Option<String>,
) {
    *state.latest_report.write().await = Some(report);
    *state.repo_url.write().await = repo_url;
    *state.repo_path.write().await = repo_path;
}

async fn health(State(state): State<AppState>) -> Json<Value> {
    let graph = state.engine.architecture_graph_snapshot().await;
    Json(json!({
        "status": "healthy",
        "service": "ckb-reality-bridge",
        "stage": "explorer-contract-v2",
        "nodes": graph.node_count(),
        "edges": graph.edge_count(),
        "localScanEnabled": state.allow_local_scan,
        "evidencePolicy": "static-runtime-predicted-separated",
        "synthetic": false
    }))
}

async fn scan_local(
    State(state): State<AppState>,
    Json(request): Json<ScanRequest>,
) -> Result<Json<Value>, (StatusCode, String)> {
    if !state.allow_local_scan {
        return Err((
            StatusCode::FORBIDDEN,
            "Local filesystem scans are disabled on this deployment".into(),
        ));
    }
    let _permit = state.scan_gate.acquire().await.map_err(internal)?;
    state.engine.reset_architecture_graph().await;
    let report = state.engine.scan_codebase(&request.path).await.map_err(internal)?;
    let project = project_label(request.project_id.as_deref(), request.repo_name.as_deref());
    remember_scan(&state, report.clone(), None, Some(request.path)).await;

    Ok(Json(json!({
        "status": "success",
        "projectKey": project,
        "filesProcessed": report.files_processed,
        "nodes": report.nodes,
        "edges": report.edges,
        "violationsFound": report.drift.len(),
        "snapshotId": report.snapshot_id,
        "source": "local-filesystem",
        "synthetic": false
    })))
}

async fn scan_github(
    State(state): State<AppState>,
    Json(request): Json<GitHubScanRequest>,
) -> Result<Json<Value>, (StatusCode, String)> {
    let _permit = state.scan_gate.acquire().await.map_err(internal)?;
    let (owner, repo) = parse_github_url(&request.github_url)
        .ok_or_else(|| bad("Invalid GitHub repository URL"))?;
    let archive_url = format!("https://api.github.com/repos/{owner}/{repo}/zipball/HEAD");

    let mut fetch = state
        .http
        .get(archive_url)
        .header("Accept", "application/vnd.github+json")
        .header("User-Agent", "CKB-Software-Reality")
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
            "Repository not found or the supplied GitHub token lacks access".into(),
        ));
    }
    if response.status() == reqwest::StatusCode::UNAUTHORIZED
        || response.status() == reqwest::StatusCode::FORBIDDEN
    {
        return Err((StatusCode::FORBIDDEN, "GitHub repository access denied".into()));
    }
    if !response.status().is_success() {
        return Err((
            StatusCode::BAD_GATEWAY,
            format!("GitHub returned {}", response.status()),
        ));
    }
    if response
        .content_length()
        .map(|size| size > MAX_ARCHIVE_BYTES as u64)
        .unwrap_or(false)
    {
        return Err((StatusCode::PAYLOAD_TOO_LARGE, "GitHub archive exceeds scan limit".into()));
    }

    let bytes = response.bytes().await.map_err(internal)?;
    if bytes.len() > MAX_ARCHIVE_BYTES {
        return Err((StatusCode::PAYLOAD_TOO_LARGE, "GitHub archive exceeds scan limit".into()));
    }

    let temp = std::env::temp_dir().join(format!("ckb-reality-{}", uuid::Uuid::new_v4()));
    let archive = bytes.to_vec();
    let extraction_target = temp.clone();
    let repo_root = tokio::task::spawn_blocking(move || {
        extract_zip_safely(&archive, &extraction_target)
    })
    .await
    .map_err(internal)?
    .map_err(bad)?;

    state.engine.reset_architecture_graph().await;
    let root_string = repo_root.to_string_lossy().to_string();
    let scan_result = state.engine.scan_codebase(&root_string).await;
    let _ = tokio::fs::remove_dir_all(&temp).await;
    let report = scan_result.map_err(internal)?;
    let canonical_url = format!("https://github.com/{owner}/{repo}");
    let project = project_label(request.project_id.as_deref(), request.repo_name.as_deref());
    remember_scan(&state, report.clone(), Some(canonical_url.clone()), None).await;

    Ok(Json(json!({
        "status": "success",
        "projectKey": project,
        "filesProcessed": report.files_processed,
        "nodes": report.nodes,
        "edges": report.edges,
        "violationsFound": report.drift.len(),
        "snapshotId": report.snapshot_id,
        "repoUrl": canonical_url,
        "source": "github-zipball",
        "engine": "tree-sitter-rust-reality-bridge",
        "synthetic": false
    })))
}

async fn scan_zip(
    State(state): State<AppState>,
    Json(request): Json<ZipScanRequest>,
) -> Result<Json<Value>, (StatusCode, String)> {
    let _permit = state.scan_gate.acquire().await.map_err(internal)?;
    let encoded = request.file_data.rsplit(',').next().unwrap_or(&request.file_data);
    let bytes = BASE64.decode(encoded.as_bytes()).map_err(bad)?;
    if bytes.len() > MAX_ARCHIVE_BYTES {
        return Err((StatusCode::PAYLOAD_TOO_LARGE, "ZIP archive exceeds scan limit".into()));
    }

    let temp = std::env::temp_dir().join(format!("ckb-zip-{}", uuid::Uuid::new_v4()));
    let extraction_target = temp.clone();
    let repo_root = tokio::task::spawn_blocking(move || {
        extract_zip_safely(&bytes, &extraction_target)
    })
    .await
    .map_err(internal)?
    .map_err(bad)?;

    state.engine.reset_architecture_graph().await;
    let root_string = repo_root.to_string_lossy().to_string();
    let scan_result = state.engine.scan_codebase(&root_string).await;
    let _ = tokio::fs::remove_dir_all(&temp).await;
    let report = scan_result.map_err(internal)?;
    let project = project_label(request.project_id.as_deref(), request.repo_name.as_deref());
    let file_name = request.file_name.unwrap_or_else(|| "upload.zip".into());
    remember_scan(&state, report.clone(), None, None).await;

    Ok(Json(json!({
        "status": "success",
        "projectKey": project,
        "filesProcessed": report.files_processed,
        "nodes": report.nodes,
        "edges": report.edges,
        "violationsFound": report.drift.len(),
        "snapshotId": report.snapshot_id,
        "fileName": file_name,
        "source": "zip-upload",
        "engine": "tree-sitter-rust-reality-bridge",
        "synthetic": false
    })))
}

async fn report(
    State(state): State<AppState>,
) -> Result<Json<ScanReport>, (StatusCode, String)> {
    let report = state.latest_report.read().await.clone();
    report
        .map(Json)
        .ok_or((StatusCode::NOT_FOUND, "No scan report is active in this process".into()))
}

async fn graph(
    State(state): State<AppState>,
) -> Result<Json<Value>, (StatusCode, String)> {
    let graph = state.engine.architecture_graph_snapshot().await;
    let snapshot_id = current_snapshot_id(&state).await;
    Ok(Json(json!({
        "graph": graph_value(&graph),
        "snapshotId": snapshot_id,
        "generatedAt": chrono::Utc::now().to_rfc3339(),
        "persistence": "sled-snapshots",
        "evidencePolicy": "static-runtime-predicted-separated",
        "synthetic": false
    })))
}

async fn impact(
    State(state): State<AppState>,
    Json(request): Json<ImpactRequest>,
) -> Result<Json<Value>, (StatusCode, String)> {
    let result = state
        .engine
        .analyze_impact(
            &request.file,
            request.line.unwrap_or(1),
            parse_change_type(request.change_type.as_deref()),
        )
        .await
        .map_err(internal)?;

    Ok(Json(json!({
        "kind": "predicted",
        "confidencePolicy": "derived-per-path",
        "assumptions": ["The current persisted AST/dependency graph is the baseline."],
        "evidence": [{
            "source": "ast-graph",
            "ref": format!("{}:{}", request.file, request.line.unwrap_or(1))
        }],
        "result": result,
        "synthetic": false
    })))
}

async fn otlp(
    State(state): State<AppState>,
    Json(request): Json<OtlpRequest>,
) -> Result<Json<Value>, (StatusCode, String)> {
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

    let summary = state
        .engine
        .ingest_otlp_spans_exact(&raw)
        .await
        .map_err(internal)?;

    Ok(Json(json!({
        "status": "observed",
        "kind": "runtime",
        "spansIngested": summary.spans_ingested,
        "nodesUpdated": summary.nodes_updated,
        "errorSpans": summary.error_spans,
        "hotpathNodes": summary.hotpath_nodes,
        "evidence": [{"source": "otlp", "ref": "ingested-payload"}],
        "synthetic": false
    })))
}

async fn runtime(
    State(state): State<AppState>,
) -> Json<Value> {
    let graph = state.engine.architecture_graph_snapshot().await;
    let nodes = graph
        .nodes()
        .into_iter()
        .filter_map(|node| {
            graph.get_runtime_metrics(&node.id).map(|metrics| {
                json!({
                    "id": node.id.0,
                    "invocationCount": metrics.execution_count,
                    "avgLatencyMs": metrics.avg_latency_ms,
                    "errorRate": metrics.error_rate,
                    "isHotpath": metrics.is_hotpath,
                    "kind": "runtime",
                    "evidence": [{"source": "otlp", "ref": node.id.0}]
                })
            })
        })
        .collect::<Vec<_>>();

    Json(json!({
        "observed": !nodes.is_empty(),
        "nodes": nodes,
        "edges": [],
        "edgeEvidenceAvailable": false,
        "note": "Node-level OTLP evidence is preserved. Parent-child runtime edge reconstruction is not enabled in this bridge yet.",
        "synthetic": false
    }))
}

async fn traces() -> Json<Value> {
    Json(json!({
        "kind": "runtime",
        "observed": false,
        "unavailable": true,
        "traces": [],
        "reason": "Parent-child OTLP trace-edge persistence is not enabled in this bridge; no trace path is synthesized.",
        "synthetic": false
    }))
}

async fn source(
    State(state): State<AppState>,
    Query(query): Query<HashMap<String, String>>,
) -> Result<Json<Value>, (StatusCode, String)> {
    let requested = query
        .get("node_id")
        .ok_or((StatusCode::BAD_REQUEST, "node_id is required".into()))?;
    let graph = state.engine.architecture_graph_snapshot().await;
    let resolved = resolve_node_id(&graph, requested)
        .ok_or((StatusCode::NOT_FOUND, "Node could not be resolved uniquely".into()))?;
    let node = graph
        .nodes()
        .into_iter()
        .find(|node| node.id == resolved)
        .ok_or((StatusCode::NOT_FOUND, "Node not found".into()))?;

    Ok(Json(json!({
        "id": node.id.0,
        "name": node.name,
        "kind": kind(node.kind),
        "path": node.path,
        "line": node.line,
        "column": node.column,
        "span": {
            "startLine": node.metadata.get("start_line"),
            "startColumn": node.metadata.get("start_column"),
            "endLine": node.metadata.get("end_line"),
            "endColumn": node.metadata.get("end_column"),
            "byteStart": node.metadata.get("byte_start"),
            "byteEnd": node.metadata.get("byte_end")
        },
        "kindOfEvidence": "static",
        "confidence": 1.0,
        "evidence": [{"source": "tree-sitter-ast", "ref": node.id.0}],
        "sourceTextIncluded": false,
        "synthetic": false
    })))
}

async fn snapshots(
    State(state): State<AppState>,
) -> Result<Json<Value>, (StatusCode, String)> {
    let mut snapshots = state
        .engine
        .architecture_snapshot_metadata()
        .await
        .map_err(internal)?;
    snapshots.sort_by(|a, b| a.timestamp.cmp(&b.timestamp));

    Ok(Json(json!({
        "snapshots": snapshots,
        "source": "ckb-persistent-sled-snapshots",
        "synthetic": false
    })))
}

async fn snapshot(
    State(state): State<AppState>,
    Query(query): Query<HashMap<String, String>>,
) -> Result<Json<Value>, (StatusCode, String)> {
    let id = query
        .get("snapshot_id")
        .ok_or((StatusCode::BAD_REQUEST, "snapshot_id is required".into()))?;
    let graph = state
        .engine
        .architecture_snapshot_graph(id)
        .await
        .map_err(internal)?
        .ok_or((StatusCode::NOT_FOUND, "Snapshot not found".into()))?;

    Ok(Json(json!({
        "kind": "static",
        "source": "ckb-persistent-sled-snapshot",
        "snapshotId": id,
        "graph": graph_value(&graph),
        "synthetic": false
    })))
}

fn edge_identity(graph: &DependencyGraph) -> HashSet<String> {
    graph
        .edges()
        .into_iter()
        .map(|edge| {
            format!("{}->{}/{:?}", edge.from.0, edge.to.0, edge.kind).to_ascii_lowercase()
        })
        .collect()
}

async fn diff(
    State(state): State<AppState>,
    Json(request): Json<DiffRequest>,
) -> Result<Json<Value>, (StatusCode, String)> {
    let from = state
        .engine
        .architecture_snapshot_graph(&request.from_snapshot)
        .await
        .map_err(internal)?
        .ok_or((StatusCode::NOT_FOUND, "from_snapshot not found".into()))?;
    let to = state
        .engine
        .architecture_snapshot_graph(&request.to_snapshot)
        .await
        .map_err(internal)?
        .ok_or((StatusCode::NOT_FOUND, "to_snapshot not found".into()))?;

    let from_nodes: HashSet<String> = from
        .nodes()
        .into_iter()
        .map(|node| node.id.0.clone())
        .collect();
    let to_nodes: HashSet<String> = to
        .nodes()
        .into_iter()
        .map(|node| node.id.0.clone())
        .collect();
    let from_edges = edge_identity(&from);
    let to_edges = edge_identity(&to);

    let mut added_nodes = to_nodes.difference(&from_nodes).cloned().collect::<Vec<_>>();
    let mut removed_nodes = from_nodes.difference(&to_nodes).cloned().collect::<Vec<_>>();
    let mut added_edges = to_edges.difference(&from_edges).cloned().collect::<Vec<_>>();
    let mut removed_edges = from_edges.difference(&to_edges).cloned().collect::<Vec<_>>();
    added_nodes.sort();
    removed_nodes.sort();
    added_edges.sort();
    removed_edges.sort();

    Ok(Json(json!({
        "kind": "static",
        "source": "persistent-snapshot-diff",
        "from": request.from_snapshot,
        "to": request.to_snapshot,
        "addedNodes": added_nodes,
        "removedNodes": removed_nodes,
        "addedEdges": added_edges,
        "removedEdges": removed_edges,
        "summary": {
            "nodeDelta": to_nodes.len() as i64 - from_nodes.len() as i64,
            "edgeDelta": to_edges.len() as i64 - from_edges.len() as i64
        },
        "evidence": [
            {"source": "ckb-snapshot", "ref": request.from_snapshot},
            {"source": "ckb-snapshot", "ref": request.to_snapshot}
        ],
        "synthetic": false
    })))
}

async fn github_history(state: &AppState, repo_url: &str, max_commits: usize) -> anyhow::Result<Value> {
    let (owner, repo) = parse_github_url(repo_url)
        .ok_or_else(|| anyhow::anyhow!("Invalid persisted GitHub URL"))?;
    let url = format!(
        "https://api.github.com/repos/{owner}/{repo}/commits?per_page={}",
        max_commits.clamp(1, 100)
    );
    let response = state
        .http
        .get(url)
        .header("Accept", "application/vnd.github+json")
        .header("User-Agent", "CKB-Software-Reality")
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
                "commitHash": value.get("sha").and_then(Value::as_str).unwrap_or(""),
                "author": value.pointer("/commit/author/name").and_then(Value::as_str).unwrap_or(""),
                "date": value.pointer("/commit/author/date").and_then(Value::as_str).unwrap_or(""),
                "message": value.pointer("/commit/message").and_then(Value::as_str).unwrap_or(""),
                "htmlUrl": value.get("html_url").and_then(Value::as_str).unwrap_or("")
            })
        })
        .collect::<Vec<_>>();

    Ok(json!({
        "kind": "history",
        "source": "github-commit-api",
        "commitsAnalyzed": entries.len(),
        "entries": entries,
        "riskMetricsIncluded": false,
        "note": "Commit identity, author, date and message are observed GitHub history. No architectural risk is invented without commit-by-commit graph rescans.",
        "synthetic": false
    }))
}

async fn history(
    State(state): State<AppState>,
    Query(query): Query<HashMap<String, String>>,
) -> Result<Json<Value>, (StatusCode, String)> {
    let max_commits = query
        .get("max_commits")
        .and_then(|value| value.parse::<usize>().ok())
        .unwrap_or(50)
        .min(500);

    if let Some(path) = state.repo_path.read().await.clone() {
        let timeline = state
            .engine
            .get_drift_timeline(&path, max_commits)
            .await
            .map_err(internal)?;
        return Ok(Json(json!({
            "kind": "history",
            "source": "git",
            "timeline": timeline,
            "evidence": [{"source": "git-history", "ref": path}],
            "synthetic": false
        })));
    }

    if let Some(url) = state.repo_url.read().await.clone() {
        return github_history(&state, &url, max_commits)
            .await
            .map(Json)
            .map_err(internal);
    }

    Ok(Json(json!({
        "kind": "history",
        "unavailable": true,
        "reason": "No repository history source is attached to the active scan.",
        "synthetic": false
    })))
}

async fn test_gaps(
    State(state): State<AppState>,
) -> Result<Json<Value>, (StatusCode, String)> {
    let report = state
        .engine
        .analyze_test_coverage_gaps()
        .await
        .map_err(internal)?;
    Ok(Json(serde_json::to_value(report).map_err(internal)?))
}

async fn rules(
    State(state): State<AppState>,
) -> Result<String, (StatusCode, String)> {
    state.engine.generate_ai_rules().await.map_err(internal)
}

async fn code_dna(
    State(state): State<AppState>,
) -> Result<Json<Value>, (StatusCode, String)> {
    let graph = state.engine.architecture_graph_snapshot().await;
    let report = ArchitectureMemoryEngine::code_dna(&graph).map_err(internal)?;
    Ok(Json(serde_json::to_value(report).map_err(internal)?))
}

async fn memory_query(
    State(state): State<AppState>,
    Json(request): Json<MemoryQueryRequest>,
) -> Result<Json<Value>, (StatusCode, String)> {
    let graph = state.engine.architecture_graph_snapshot().await;
    let report = ArchitectureMemoryEngine::query(
        &graph,
        &request.query,
        request.depth.unwrap_or(2),
        request.limit.unwrap_or(12),
    )
    .map_err(internal)?;
    Ok(Json(serde_json::to_value(report).map_err(internal)?))
}

async fn causal_path(
    State(state): State<AppState>,
    Json(request): Json<CausalPathRequest>,
) -> Result<Json<Value>, (StatusCode, String)> {
    let graph = state.engine.architecture_graph_snapshot().await;
    let source = resolve_node_id(&graph, &request.source)
        .ok_or((StatusCode::NOT_FOUND, "source node could not be resolved uniquely".into()))?;
    let target = resolve_node_id(&graph, &request.target)
        .ok_or((StatusCode::NOT_FOUND, "target node could not be resolved uniquely".into()))?;
    let report = CausalArchitectureEngine::shortest_path(
        &graph,
        &source,
        &target,
        request.max_depth.unwrap_or(12),
    )
    .map_err(internal)?;
    Ok(Json(serde_json::to_value(report).map_err(internal)?))
}

async fn failure_cone(
    State(state): State<AppState>,
    Json(request): Json<FailureConeRequest>,
) -> Result<Json<Value>, (StatusCode, String)> {
    let graph = state.engine.architecture_graph_snapshot().await;
    let root = resolve_node_id(&graph, &request.root)
        .ok_or((StatusCode::NOT_FOUND, "root node could not be resolved uniquely".into()))?;
    let report = CausalArchitectureEngine::failure_cone(
        &graph,
        &root,
        request.max_depth.unwrap_or(12),
    )
    .map_err(internal)?;
    Ok(Json(serde_json::to_value(report).map_err(internal)?))
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt::init();

    let data_dir = PathBuf::from(
        std::env::var("CKB_REALITY_DATA_DIR")
            .unwrap_or_else(|_| "./ckb_reality_data".into()),
    );
    tokio::fs::create_dir_all(&data_dir).await?;
    let graph_store = data_dir.join("graph_store");
    let graph_store_string = graph_store.to_string_lossy().to_string();

    let http = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(120))
        .build()?;
    let state = AppState {
        engine: Arc::new(CkbEngine::new_with_storage_path(&graph_store_string)?),
        latest_report: Arc::new(RwLock::new(None)),
        repo_url: Arc::new(RwLock::new(None)),
        repo_path: Arc::new(RwLock::new(None)),
        scan_gate: Arc::new(Semaphore::new(1)),
        http,
        allow_local_scan: std::env::var("CKB_ALLOW_LOCAL_SCAN")
            .map(|value| value == "1" || value.eq_ignore_ascii_case("true"))
            .unwrap_or(false),
    };

    let app = Router::new()
        .route("/health", get(health))
        .route("/api/v1/scan", post(scan_local))
        .route("/api/v1/report", get(report))
        .route("/api/v1/impact", post(impact))
        .route("/api/v1/otlp", post(otlp))
        .route("/api/v1/test-gaps", get(test_gaps))
        .route("/api/v1/rules", get(rules))
        .route("/api/v1/intelligence/scan/github", post(scan_github))
        .route("/api/v1/intelligence/scan/zip", post(scan_zip))
        .route("/api/v1/intelligence/graph", get(graph))
        .route("/api/v1/intelligence/impact", post(impact))
        .route("/api/v1/intelligence/telemetry/otlp", post(otlp))
        .route("/api/v1/intelligence/runtime", get(runtime))
        .route("/api/v1/intelligence/traces", get(traces))
        .route("/api/v1/intelligence/source", get(source))
        .route("/api/v1/intelligence/history", get(history))
        .route("/api/v1/intelligence/snapshots", get(snapshots))
        .route("/api/v1/intelligence/snapshot", get(snapshot))
        .route("/api/v1/intelligence/diff", post(diff))
        .route("/api/v1/intelligence/code-dna", get(code_dna))
        .route("/api/v1/intelligence/memory/query", post(memory_query))
        .route("/api/v1/intelligence/causal-path", post(causal_path))
        .route("/api/v1/intelligence/failure-cone", post(failure_cone))
        .layer(DefaultBodyLimit::max(85 * 1024 * 1024))
        .layer(
            CorsLayer::new()
                .allow_origin(Any)
                .allow_methods(Any)
                .allow_headers(Any),
        )
        .with_state(state);

    let port: u16 = std::env::var("PORT")
        .ok()
        .and_then(|value| value.parse::<u16>().ok())
        .unwrap_or(3000);
    let address = SocketAddr::from(([0, 0, 0, 0], port));
    let listener = tokio::net::TcpListener::bind(address).await?;
    axum::serve(listener, app).await?;
    Ok(())
}
