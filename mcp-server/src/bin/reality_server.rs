use axum::{
    extract::{Query, State},
    http::{HeaderMap, StatusCode},
    middleware::{self, Next},
    response::Response,
    routing::{get, post},
    Json, Router,
};
use ckb_core::{
    ChangeType, CkbEngine, DependencyGraph, DynamicTrace, GraphStorage, Node, NodeId,
    OtlpReceiver, RuntimeMetrics, ScanReport,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{collections::HashMap, sync::Arc};
use tokio::sync::RwLock;
use tower_http::cors::{Any, CorsLayer};
use tracing::{info, warn};

#[derive(Clone)]
struct Session {
    engine: Arc<RwLock<CkbEngine>>,
    report: Arc<RwLock<Option<ScanReport>>>,
    repo_path: Arc<RwLock<Option<String>>>,
    runtime_nodes: Arc<RwLock<HashMap<NodeId, RuntimeMetrics>>>,
    runtime_edges: Arc<RwLock<HashMap<String, RuntimeEdgeObservation>>>,
}

impl Session {
    fn new() -> anyhow::Result<Self> {
        Ok(Self {
            engine: Arc::new(RwLock::new(CkbEngine::new()?)),
            report: Arc::new(RwLock::new(None)),
            repo_path: Arc::new(RwLock::new(None)),
            runtime_nodes: Arc::new(RwLock::new(HashMap::new())),
            runtime_edges: Arc::new(RwLock::new(HashMap::new())),
        })
    }
}

#[derive(Clone)]
struct AppState {
    default_session: Session,
    sessions: Arc<RwLock<HashMap<String, Session>>>,
    storage: Arc<GraphStorage>,
    api_key: Option<Arc<String>>,
}

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

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct IntelligenceEnvelope {
    kind: &'static str,
    confidence: f32,
    evidence: Vec<Value>,
    explanation: String,
    observed_at: Option<u64>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct IntelligenceNode {
    id: String,
    name: String,
    kind: String,
    path: String,
    line: u32,
    column: u32,
    metadata: HashMap<String, String>,
    runtime: Option<RuntimeNodeDto>,
    intelligence: IntelligenceEnvelope,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct RuntimeNodeDto {
    invocation_count: u64,
    avg_latency_ms: f32,
    error_rate: f32,
    is_hotpath: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct IntelligenceLink {
    id: String,
    source: String,
    target: String,
    kind: String,
    weight: f32,
    metadata: HashMap<String, String>,
    runtime: Option<RuntimeLinkDto>,
    intelligence: IntelligenceEnvelope,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct RuntimeLinkDto {
    invocation_count: u64,
    avg_latency_ms: f64,
    error_rate: f64,
    last_seen_unix_nano: u64,
    trace_id: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct IntelligenceGraphResponse {
    graph: GraphDto,
    snapshot_id: String,
    generated_at: String,
}

#[derive(Debug, Serialize)]
struct GraphDto {
    nodes: Vec<IntelligenceNode>,
    links: Vec<IntelligenceLink>,
}

#[derive(Debug, Deserialize)]
struct ScanRequest {
    path: String,
    repo_name: Option<String>,
    project_id: Option<String>,
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

fn default_line() -> u32 { 1 }

#[derive(Debug, Deserialize)]
struct OtlpRequest {
    raw_spans: Option<String>,
    otlp_json: Option<String>,
    payload: Option<Value>,
    repo_name: Option<String>,
    project_id: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct RuntimeResponse {
    nodes: Vec<Value>,
    edges: Vec<Value>,
    observed: bool,
}

fn session_key(repo_name: Option<&str>, project_id: Option<&str>) -> Option<String> {
    project_id.filter(|s| !s.is_empty()).or_else(|| repo_name.filter(|s| !s.is_empty())).map(str::to_string)
}

async fn resolve_session(state: &AppState, key: Option<String>) -> Result<Session, (StatusCode, String)> {
    let Some(key) = key else { return Ok(state.default_session.clone()); };
    if let Some(session) = state.sessions.read().await.get(&key) { return Ok(session.clone()); }
    let mut sessions = state.sessions.write().await;
    if let Some(session) = sessions.get(&key) { return Ok(session.clone()); }
    let session = Session::new().map_err(internal)?;
    sessions.insert(key, session.clone());
    Ok(session)
}

fn internal<E: std::fmt::Display>(e: E) -> (StatusCode, String) {
    (StatusCode::INTERNAL_SERVER_ERROR, e.to_string())
}

fn extract_key(headers: &HeaderMap) -> Option<String> {
    headers.get("x-api-key").and_then(|v| v.to_str().ok()).map(str::to_string)
        .or_else(|| headers.get(axum::http::header::AUTHORIZATION)
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.strip_prefix("Bearer "))
            .map(str::to_string))
}

async fn require_api_key(
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

async fn health() -> Json<Value> {
    Json(json!({
        "status": "healthy",
        "service": "ckb-reality-server",
        "realityApi": "v2",
        "evidencePolicy": "static-runtime-predicted-separated"
    }))
}

async fn scan(State(state): State<AppState>, Json(req): Json<ScanRequest>) -> Result<Json<Value>, (StatusCode, String)> {
    let key = session_key(req.repo_name.as_deref(), req.project_id.as_deref());
    let session = resolve_session(&state, key).await?;
    let report = session.engine.read().await.scan_codebase(&req.path).await.map_err(internal)?;
    *session.repo_path.write().await = Some(req.path);
    *session.report.write().await = Some(report.clone());
    session.runtime_nodes.write().await.clear();
    session.runtime_edges.write().await.clear();
    Ok(Json(json!({
        "status": "success",
        "filesProcessed": report.files_processed,
        "nodes": report.nodes,
        "edges": report.edges,
        "violationsFound": report.drift.len(),
        "snapshotId": report.snapshot_id
    })))
}

async fn report(State(state): State<AppState>, Query(q): Query<HashMap<String, String>>) -> Result<Json<ScanReport>, (StatusCode, String)> {
    let key = session_key(q.get("repo").map(String::as_str), q.get("project_id").map(String::as_str));
    let session = resolve_session(&state, key).await?;
    session.report.read().await.clone().map(Json).ok_or((StatusCode::NOT_FOUND, "No scan has been run for this session".into()))
}

fn change_type(value: Option<&str>) -> ChangeType {
    match value.unwrap_or("modify").to_ascii_lowercase().as_str() {
        "add" => ChangeType::Add,
        "delete" => ChangeType::Delete,
        "rename" => ChangeType::Rename,
        _ => ChangeType::Modify,
    }
}

async fn impact(State(state): State<AppState>, Json(req): Json<ImpactRequest>) -> Result<Json<Value>, (StatusCode, String)> {
    let key = session_key(req.repo_name.as_deref(), req.project_id.as_deref());
    let session = resolve_session(&state, key).await?;
    if session.report.read().await.is_none() {
        let path = req.path.as_deref().ok_or((StatusCode::PRECONDITION_REQUIRED, "Scan the project first or provide path".into()))?;
        let r = session.engine.read().await.scan_codebase(path).await.map_err(internal)?;
        *session.repo_path.write().await = Some(path.to_string());
        *session.report.write().await = Some(r);
    }
    let result = session.engine.read().await.analyze_impact(&req.file, req.line, change_type(req.change_type.as_deref())).await.map_err(internal)?;
    Ok(Json(json!({
        "kind": "predicted",
        "confidencePolicy": "derived-per-path",
        "assumptions": ["Current scanned graph represents the proposed change baseline"],
        "evidence": [{"source":"ast-graph","ref": format!("{}:{}", req.file, req.line)}],
        "result": result
    })))
}

fn kind_name<T: std::fmt::Debug>(v: T) -> String { format!("{:?}", v).to_ascii_lowercase() }

async fn load_graph(state: &AppState, session: &Session) -> Result<(DependencyGraph, String), (StatusCode, String)> {
    let report = session.report.read().await.clone().ok_or((StatusCode::PRECONDITION_REQUIRED, "No scan has been run for this session".into()))?;
    let graph = state.storage.load_snapshot(&report.snapshot_id).await.map_err(internal)?
        .ok_or((StatusCode::NOT_FOUND, "Graph snapshot is unavailable".into()))?;
    Ok((graph, report.snapshot_id))
}

fn runtime_for_id(runtime: &HashMap<NodeId, RuntimeMetrics>, node: &Node) -> Option<RuntimeMetrics> {
    if let Some(m) = runtime.get(&node.id) { return Some(m.clone()); }
    let node_path = node.path.to_string_lossy().replace('\\', "/");
    runtime.iter().find_map(|(id, m)| {
        let raw = id.0.replace('\\', "/");
        if raw == node.name || raw.ends_with(&format!("::{}", node.name)) || raw.starts_with(&format!("{}::", node_path)) && raw.ends_with(&node.name) {
            Some(m.clone())
        } else { None }
    })
}

async fn intelligence_graph(State(state): State<AppState>, Query(q): Query<HashMap<String, String>>) -> Result<Json<IntelligenceGraphResponse>, (StatusCode, String)> {
    let key = session_key(q.get("repo").map(String::as_str), q.get("project_id").map(String::as_str));
    let session = resolve_session(&state, key).await?;
    let (graph, snapshot_id) = load_graph(&state, &session).await?;
    let runtime_nodes = session.runtime_nodes.read().await.clone();
    let runtime_edges = session.runtime_edges.read().await.clone();

    let nodes = graph.nodes().into_iter().map(|n| {
        let runtime = runtime_for_id(&runtime_nodes, n);
        let evidence_ref = format!("{}:{}:{}", n.path.to_string_lossy(), n.line, n.column);
        IntelligenceNode {
            id: n.id.0.clone(),
            name: n.name.clone(),
            kind: kind_name(n.kind),
            path: n.path.to_string_lossy().to_string(),
            line: n.line,
            column: n.column,
            metadata: n.metadata.clone(),
            runtime: runtime.as_ref().map(|m| RuntimeNodeDto {
                invocation_count: m.execution_count,
                avg_latency_ms: m.avg_latency_ms,
                error_rate: m.error_rate,
                is_hotpath: m.is_hotpath,
            }),
            intelligence: IntelligenceEnvelope {
                kind: if runtime.is_some() { "runtime" } else { "static" },
                confidence: 1.0,
                evidence: vec![json!({"source":"tree-sitter-ast","ref":evidence_ref})],
                explanation: if runtime.is_some() { "Static source symbol with runtime-observed telemetry overlay.".into() } else { "Source symbol discovered from the CKB AST graph.".into() },
                observed_at: None,
            },
        }
    }).collect();

    let links = graph.edges().into_iter().map(|e| {
        let key = format!("{}->{}", e.from.0, e.to.0);
        let observed = runtime_edges.get(&key);
        IntelligenceLink {
            id: e.id.to_string(),
            source: e.from.0.clone(),
            target: e.to.0.clone(),
            kind: kind_name(e.kind),
            weight: e.weight,
            metadata: e.metadata.clone(),
            runtime: observed.map(|r| RuntimeLinkDto {
                invocation_count: r.invocation_count,
                avg_latency_ms: r.avg_latency_ms(),
                error_rate: r.error_rate(),
                last_seen_unix_nano: r.last_seen_unix_nano,
                trace_id: r.trace_id.clone(),
            }),
            intelligence: IntelligenceEnvelope {
                kind: if observed.is_some() { "runtime" } else { "static" },
                confidence: 1.0,
                evidence: vec![json!({"source": if observed.is_some() {"otlp+ast"} else {"ast-graph"}, "ref":key})],
                explanation: if observed.is_some() { "Static relationship confirmed by an observed parent/child OTLP execution path.".into() } else { "Structural relationship discovered from source analysis.".into() },
                observed_at: observed.map(|r| r.last_seen_unix_nano),
            },
        }
    }).collect();

    Ok(Json(IntelligenceGraphResponse {
        graph: GraphDto { nodes, links },
        snapshot_id,
        generated_at: chrono::Utc::now().to_rfc3339(),
    }))
}

async fn runtime(State(state): State<AppState>, Query(q): Query<HashMap<String, String>>) -> Result<Json<RuntimeResponse>, (StatusCode, String)> {
    let key = session_key(q.get("repo").map(String::as_str), q.get("project_id").map(String::as_str));
    let session = resolve_session(&state, key).await?;
    let nodes = session.runtime_nodes.read().await.iter().map(|(id, m)| json!({
        "id": id.0,
        "invocationCount": m.execution_count,
        "avgLatencyMs": m.avg_latency_ms,
        "errorRate": m.error_rate,
        "isHotpath": m.is_hotpath,
        "kind":"runtime",
        "evidence":[{"source":"otlp","ref":id.0}]
    })).collect::<Vec<_>>();
    let edges = session.runtime_edges.read().await.values().map(|r| json!({
        "source": r.source,
        "target": r.target,
        "traceId": r.trace_id,
        "invocationCount": r.invocation_count,
        "avgLatencyMs": r.avg_latency_ms(),
        "errorRate": r.error_rate(),
        "lastSeenUnixNano": r.last_seen_unix_nano,
        "kind":"runtime",
        "evidence":[{"source":"otlp-parent-child","ref":format!("{}->{}", r.source, r.target)}]
    })).collect::<Vec<_>>();
    Ok(Json(RuntimeResponse { observed: !nodes.is_empty() || !edges.is_empty(), nodes, edges }))
}

fn scalar(v: &Value) -> Option<String> {
    v.as_str().map(str::to_string)
        .or_else(|| v.get("stringValue").and_then(Value::as_str).map(str::to_string))
        .or_else(|| v.get("intValue").and_then(|x| x.as_str().map(str::to_string).or_else(|| x.as_u64().map(|n|n.to_string()))))
}

fn attrs(v: Option<&Value>) -> HashMap<String,String> {
    let mut out = HashMap::new();
    if let Some(items) = v.and_then(Value::as_array) {
        for item in items {
            if let (Some(k), Some(val)) = (item.get("key").and_then(Value::as_str), item.get("value").and_then(scalar)) { out.insert(k.into(), val); }
        }
    } else if let Some(obj) = v.and_then(Value::as_object) {
        for (k,v) in obj { if let Some(val) = scalar(v) { out.insert(k.clone(), val); } }
    }
    out
}

fn u64v(v: Option<&Value>) -> u64 { v.and_then(Value::as_u64).or_else(||v.and_then(Value::as_str).and_then(|s|s.parse().ok())).unwrap_or(0) }

fn collect_spans(root: &Value) -> Vec<Value> {
    if let Some(a) = root.as_array() { return a.clone(); }
    let mut spans = Vec::new();
    if let Some(resources) = root.get("resourceSpans").and_then(Value::as_array) {
        for resource in resources {
            let resource_attrs = attrs(resource.get("resource").and_then(|r|r.get("attributes")));
            if let Some(scopes) = resource.get("scopeSpans").or_else(||resource.get("instrumentationLibrarySpans")).and_then(Value::as_array) {
                for scope in scopes {
                    if let Some(items) = scope.get("spans").and_then(Value::as_array) {
                        for raw in items {
                            let mut span = raw.clone();
                            if let Some(obj) = span.as_object_mut() {
                                let mut merged = resource_attrs.clone();
                                merged.extend(attrs(raw.get("attributes")));
                                obj.insert("_ckbMergedAttributes".into(), serde_json::to_value(merged).unwrap_or(Value::Null));
                            }
                            spans.push(span);
                        }
                    }
                }
            }
        }
    }
    spans
}

fn canonical_span_id(span: &Value) -> String {
    let a: HashMap<String,String> = span.get("_ckbMergedAttributes").and_then(|v|serde_json::from_value(v.clone()).ok()).unwrap_or_else(||attrs(span.get("attributes")));
    let file = a.get("code.file.path").or_else(||a.get("code.filepath")).or_else(||a.get("code.file.name"));
    let function = a.get("code.function.name").or_else(||a.get("code.function")).or_else(||a.get("function.name"));
    let namespace = a.get("code.namespace").or_else(||a.get("service.name"));
    match (file,function,namespace) {
        (Some(f),Some(fun),_) => format!("{}::{}", f.replace('\\', "/"), fun),
        (Some(f),None,_) => format!("{}::file", f.replace('\\', "/")),
        (None,Some(fun),Some(ns)) => format!("{}::{}",ns,fun),
        (None,Some(fun),None) => fun.clone(),
        (None,None,Some(ns)) => format!("{}::{}",ns,span.get("name").and_then(Value::as_str).unwrap_or("span")),
        _ => span.get("name").and_then(Value::as_str).unwrap_or("span").to_string(),
    }
}

fn is_error(span: &Value) -> bool {
    let c = span.get("status").and_then(|s|s.get("code"));
    c.and_then(Value::as_u64).map(|n|n==2).or_else(||c.and_then(Value::as_str).map(|s|matches!(s.to_ascii_uppercase().as_str(),"2"|"ERROR"|"STATUS_CODE_ERROR"))).unwrap_or(false)
}

fn trace_edges(raw: &str) -> anyhow::Result<HashMap<String, RuntimeEdgeObservation>> {
    let root: Value = serde_json::from_str(raw)?;
    let spans = collect_spans(&root);
    let mut by_span: HashMap<String,(String,String)> = HashMap::new();
    for s in &spans {
        let sid = s.get("spanId").or_else(||s.get("span_id")).and_then(Value::as_str).unwrap_or("").to_string();
        let trace = s.get("traceId").or_else(||s.get("trace_id")).and_then(Value::as_str).unwrap_or("").to_string();
        if !sid.is_empty() { by_span.insert(sid, (trace, canonical_span_id(s))); }
    }
    let mut out = HashMap::new();
    for s in &spans {
        let parent = s.get("parentSpanId").or_else(||s.get("parent_span_id")).and_then(Value::as_str).unwrap_or("");
        if parent.is_empty() { continue; }
        let Some((parent_trace, source)) = by_span.get(parent).cloned() else { continue; };
        let target = canonical_span_id(s);
        let trace_id = s.get("traceId").or_else(||s.get("trace_id")).and_then(Value::as_str).unwrap_or(&parent_trace).to_string();
        let start = u64v(s.get("startTimeUnixNano").or_else(||s.get("start_time_unix_nano")));
        let end = u64v(s.get("endTimeUnixNano").or_else(||s.get("end_time_unix_nano")));
        let duration_ms = end.saturating_sub(start) as f64 / 1_000_000.0;
        let key = format!("{}->{}",source,target);
        let e = out.entry(key).or_insert(RuntimeEdgeObservation { source, target, trace_id, invocation_count:0,error_count:0,total_latency_ms:0.0,last_seen_unix_nano:0 });
        e.invocation_count += 1;
        e.total_latency_ms += duration_ms;
        e.last_seen_unix_nano = e.last_seen_unix_nano.max(end);
        if is_error(s) { e.error_count += 1; }
    }
    Ok(out)
}

fn merge_runtime_nodes(target: &mut HashMap<NodeId,RuntimeMetrics>, incoming: HashMap<NodeId,RuntimeMetrics>) {
    for (id,m) in incoming {
        let e = target.entry(id).or_insert(RuntimeMetrics { execution_count:0,avg_latency_ms:0.0,error_rate:0.0,is_hotpath:false });
        let old=e.execution_count; let total=old.saturating_add(m.execution_count);
        if total>0 {
            e.avg_latency_ms=((e.avg_latency_ms as f64*old as f64 + m.avg_latency_ms as f64*m.execution_count as f64)/total as f64) as f32;
            e.error_rate=((e.error_rate as f64*old as f64 + m.error_rate as f64*m.execution_count as f64)/total as f64) as f32;
        }
        e.execution_count=total; e.is_hotpath=e.is_hotpath||m.is_hotpath||total>500;
    }
}

fn merge_runtime_edges(target:&mut HashMap<String,RuntimeEdgeObservation>,incoming:HashMap<String,RuntimeEdgeObservation>){
    for (k,m) in incoming { let e=target.entry(k).or_insert_with(||m.clone()); if e.invocation_count==m.invocation_count && e.total_latency_ms==m.total_latency_ms {continue;} e.invocation_count+=m.invocation_count;e.error_count+=m.error_count;e.total_latency_ms+=m.total_latency_ms;e.last_seen_unix_nano=e.last_seen_unix_nano.max(m.last_seen_unix_nano);e.trace_id=m.trace_id; }
}

async fn ingest_otlp(State(state): State<AppState>, Json(req): Json<OtlpRequest>) -> Result<Json<Value>, (StatusCode, String)> {
    let key = session_key(req.repo_name.as_deref(), req.project_id.as_deref());
    let session = resolve_session(&state, key).await?;
    let raw = if let Some(v)=req.raw_spans.or(req.otlp_json){v}else if let Some(v)=req.payload{serde_json::to_string(&v).map_err(internal)?}else{return Err((StatusCode::BAD_REQUEST,"Provide raw_spans, otlp_json, or payload".into()));};
    let node_metrics = OtlpReceiver::ingest_spans(&raw).map_err(internal)?;
    let edge_metrics = trace_edges(&raw).map_err(internal)?;
    let summary = OtlpReceiver::summarize(&node_metrics);
    merge_runtime_nodes(&mut session.runtime_nodes.write().await,node_metrics);
    merge_runtime_edges(&mut session.runtime_edges.write().await,edge_metrics);
    // Keep the engine's own runtime layer updated too, so existing MCP/REST consumers remain consistent.
    let _ = session.engine.read().await.ingest_otlp_spans(&raw).await;
    Ok(Json(json!({
        "status":"observed",
        "kind":"runtime",
        "spansIngested":summary.spans_ingested,
        "nodesUpdated":summary.nodes_updated,
        "errorSpans":summary.error_spans,
        "hotpathNodes":summary.hotpath_nodes,
        "runtimeEdges":session.runtime_edges.read().await.len(),
        "evidence":[{"source":"otlp","ref":"ingested-payload"}]
    })))
}

async fn source_evidence(State(state): State<AppState>, Query(q): Query<HashMap<String,String>>) -> Result<Json<Value>,(StatusCode,String)> {
    let key=session_key(q.get("repo").map(String::as_str),q.get("project_id").map(String::as_str));
    let session=resolve_session(&state,key).await?;
    let node_id=q.get("node_id").ok_or((StatusCode::BAD_REQUEST,"node_id is required".into()))?;
    let (graph,_) = load_graph(&state,&session).await?;
    let node=graph.nodes().into_iter().find(|n|n.id.0==*node_id).ok_or((StatusCode::NOT_FOUND,"Node not found".into()))?;
    Ok(Json(json!({
        "id":node.id.0,"name":node.name,"kind":kind_name(node.kind),"path":node.path,"line":node.line,"column":node.column,
        "span":{"startLine":node.metadata.get("start_line"),"startColumn":node.metadata.get("start_column"),"endLine":node.metadata.get("end_line"),"endColumn":node.metadata.get("end_column"),"byteStart":node.metadata.get("byte_start"),"byteEnd":node.metadata.get("byte_end")},
        "kindOfEvidence":"static","confidence":1.0,"evidence":[{"source":"tree-sitter-ast","ref":node.id.0}]
    })))
}

async fn history(State(state): State<AppState>, Query(q): Query<HashMap<String,String>>) -> Result<Json<Value>,(StatusCode,String)> {
    let key=session_key(q.get("repo").map(String::as_str),q.get("project_id").map(String::as_str));
    let session=resolve_session(&state,key).await?;
    let repo_path=session.repo_path.read().await.clone().or_else(||q.get("path").cloned()).ok_or((StatusCode::PRECONDITION_REQUIRED,"Scan a repository first or provide path".into()))?;
    let max=q.get("max_commits").and_then(|v|v.parse::<usize>().ok()).unwrap_or(50).min(500);
    let timeline=session.engine.read().await.get_drift_timeline(&repo_path,max).await.map_err(internal)?;
    Ok(Json(json!({"kind":"static","source":"git","timeline":timeline,"evidence":[{"source":"git-history","ref":repo_path}]})))
}

async fn traces(State(state): State<AppState>, Query(q): Query<HashMap<String,String>>) -> Result<Json<Value>,(StatusCode,String)> {
    let key=session_key(q.get("repo").map(String::as_str),q.get("project_id").map(String::as_str));
    let session=resolve_session(&state,key).await?;
    let edges=session.runtime_edges.read().await;
    let mut by_trace:HashMap<String,Vec<Value>>=HashMap::new();
    for e in edges.values(){by_trace.entry(e.trace_id.clone()).or_default().push(json!({"source":e.source,"target":e.target,"invocationCount":e.invocation_count,"avgLatencyMs":e.avg_latency_ms(),"errorRate":e.error_rate(),"lastSeenUnixNano":e.last_seen_unix_nano}));}
    Ok(Json(json!({"kind":"runtime","traces":by_trace,"observed":!edges.is_empty()})))
}

async fn drift_timeline(State(state):State<AppState>,Query(q):Query<HashMap<String,String>>)->Result<Json<Value>,(StatusCode,String)>{history(State(state),Query(q)).await}

async fn test_gaps(State(state):State<AppState>,Query(q):Query<HashMap<String,String>>)->Result<Json<Value>,(StatusCode,String)>{
    let key=session_key(q.get("repo").map(String::as_str),q.get("project_id").map(String::as_str));let s=resolve_session(&state,key).await?;let r=s.engine.read().await.analyze_test_coverage_gaps().await.map_err(internal)?;Ok(Json(serde_json::to_value(r).map_err(internal)?))
}

async fn rules(State(state):State<AppState>,Query(q):Query<HashMap<String,String>>)->Result<String,(StatusCode,String)>{
    let key=session_key(q.get("repo").map(String::as_str),q.get("project_id").map(String::as_str));let s=resolve_session(&state,key).await?;s.engine.read().await.generate_ai_rules().await.map_err(internal)
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt::init();
    let default_session=Session::new()?;
    let state=AppState{
        default_session,
        sessions:Arc::new(RwLock::new(HashMap::new())),
        storage:Arc::new(GraphStorage::new("./ckb_data")?),
        api_key:std::env::var("CKB_API_KEY").ok().filter(|v|!v.is_empty()).map(Arc::new),
    };
    if state.api_key.is_none(){warn!("CKB_API_KEY is not configured; Reality API is unauthenticated.");}
    let protected=Router::new()
        .route("/api/v1/scan",post(scan))
        .route("/api/v1/report",get(report))
        .route("/api/v1/impact",post(impact))
        .route("/api/v1/otlp",post(ingest_otlp))
        .route("/api/v1/drift-timeline",get(drift_timeline))
        .route("/api/v1/test-gaps",get(test_gaps))
        .route("/api/v1/rules",get(rules))
        .route("/api/v1/intelligence/graph",get(intelligence_graph))
        .route("/api/v1/intelligence/source",get(source_evidence))
        .route("/api/v1/intelligence/runtime",get(runtime))
        .route("/api/v1/intelligence/traces",get(traces))
        .route("/api/v1/intelligence/impact",post(impact))
        .route("/api/v1/intelligence/telemetry/otlp",post(ingest_otlp))
        .route("/api/v1/intelligence/history",get(history))
        .route_layer(middleware::from_fn_with_state(state.clone(),require_api_key));
    let cors=match std::env::var("CKB_ALLOWED_ORIGIN"){
        Ok(v) if v=="*"=>CorsLayer::new().allow_origin(Any).allow_methods(Any).allow_headers(Any),
        Ok(v)=>CorsLayer::new().allow_origin(v.parse::<axum::http::HeaderValue>()?).allow_methods(Any).allow_headers(Any),
        Err(_)=>CorsLayer::new(),
    };
    let app=Router::new().route("/health",get(health)).merge(protected).layer(cors).with_state(state);
    let port=std::env::var("PORT").ok().and_then(|v|v.parse().ok()).unwrap_or(3000);
    let bind_all=std::env::var("CKB_BIND_ALL").map(|v|v=="1"||v.eq_ignore_ascii_case("true")).unwrap_or(false);
    let host=if bind_all{[0,0,0,0]}else{[127,0,0,1]};
    let addr=std::net::SocketAddr::from((host,port));
    info!("CKB Reality API listening on {}",addr);
    let listener=tokio::net::TcpListener::bind(addr).await?;axum::serve(listener,app).await?;Ok(())
}
