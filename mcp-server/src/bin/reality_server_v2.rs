use axum::{
    extract::{Query, State},
    http::{HeaderMap, StatusCode},
    middleware::{self, Next},
    response::Response,
    routing::{get, post},
    Json, Router,
};
use ckb_core::{
    ArchitectureAnalyzer, ChangeType, DependencyGraph, FileAnalysis, GitDriftAnalyzer,
    LanguageParser, Node, NodeId, OtlpReceiver, RuntimeMetrics, ScanReport,
    TestCoverageAnalyzer,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{collections::{BTreeSet, HashMap}, path::{Path, PathBuf}, sync::Arc};
use tokio::sync::RwLock;
use tower_http::cors::{Any, CorsLayer};
use tracing::{info, warn};

#[derive(Clone)]
struct Session {
    graph: Arc<RwLock<Option<DependencyGraph>>>,
    report: Arc<RwLock<Option<ScanReport>>>,
    repo_path: Arc<RwLock<Option<String>>>,
    runtime_nodes: Arc<RwLock<HashMap<NodeId, RuntimeMetrics>>>,
    runtime_edges: Arc<RwLock<HashMap<String, RuntimeEdgeObservation>>>,
}

impl Session {
    fn new() -> Self {
        Self {
            graph: Arc::new(RwLock::new(None)),
            report: Arc::new(RwLock::new(None)),
            repo_path: Arc::new(RwLock::new(None)),
            runtime_nodes: Arc::new(RwLock::new(HashMap::new())),
            runtime_edges: Arc::new(RwLock::new(HashMap::new())),
        }
    }
}

#[derive(Clone)]
struct AppState {
    default_session: Session,
    sessions: Arc<RwLock<HashMap<String, Session>>>,
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
    fn error_rate(&self) -> f64 { if self.invocation_count == 0 { 0.0 } else { self.error_count as f64 / self.invocation_count as f64 } }
    fn avg_latency_ms(&self) -> f64 { if self.invocation_count == 0 { 0.0 } else { self.total_latency_ms / self.invocation_count as f64 } }
}

#[derive(Debug, Deserialize)]
struct ScanRequest { path: String, repo_name: Option<String>, project_id: Option<String> }
#[derive(Debug, Deserialize)]
struct ImpactRequest { path: Option<String>, file: String, #[serde(default = "default_line")] line: u32, change_type: Option<String>, repo_name: Option<String>, project_id: Option<String> }
fn default_line() -> u32 { 1 }
#[derive(Debug, Deserialize)]
struct OtlpRequest { raw_spans: Option<String>, otlp_json: Option<String>, payload: Option<Value>, repo_name: Option<String>, project_id: Option<String> }

fn internal<E: std::fmt::Display>(e:E)->(StatusCode,String){(StatusCode::INTERNAL_SERVER_ERROR,e.to_string())}
fn session_key(repo:Option<&str>,project:Option<&str>)->Option<String>{project.filter(|s|!s.is_empty()).or_else(||repo.filter(|s|!s.is_empty())).map(str::to_string)}
async fn session(state:&AppState,key:Option<String>)->Session{
    let Some(key)=key else{return state.default_session.clone()};
    if let Some(s)=state.sessions.read().await.get(&key){return s.clone()}
    let mut all=state.sessions.write().await; all.entry(key).or_insert_with(Session::new).clone()
}

fn extract_key(headers:&HeaderMap)->Option<String>{
    headers.get("x-api-key").and_then(|v|v.to_str().ok()).map(str::to_string).or_else(||headers.get(axum::http::header::AUTHORIZATION).and_then(|v|v.to_str().ok()).and_then(|v|v.strip_prefix("Bearer ")).map(str::to_string))
}
async fn auth(State(state):State<AppState>,headers:HeaderMap,request:axum::http::Request<axum::body::Body>,next:Next)->Result<Response,(StatusCode,String)>{
    if let Some(expected)=&state.api_key{if extract_key(&headers).as_deref()!=Some(expected.as_str()){return Err((StatusCode::UNAUTHORIZED,"Missing or invalid CKB API key".into()))}}
    Ok(next.run(request).await)
}

fn supported(path:&Path)->bool{matches!(path.extension().and_then(|v|v.to_str()).unwrap_or(""),"ts"|"tsx"|"js"|"jsx"|"mjs"|"py"|"go"|"rs"|"java")}
fn discover(root:&str)->anyhow::Result<Vec<PathBuf>>{
    let mut out=Vec::new();let mut stack=vec![PathBuf::from(root)];
    while let Some(dir)=stack.pop(){
        for entry in std::fs::read_dir(&dir)?{let entry=entry?;let p=entry.path();let name=p.file_name().and_then(|v|v.to_str()).unwrap_or("");
            if p.is_dir(){if !matches!(name,".git"|"node_modules"|"target"|"dist"|"build"|".next"|"vendor"){stack.push(p)}}else if supported(&p){out.push(p)}
        }
    }
    Ok(out)
}
fn package_identity(root:&str)->Option<String>{
    let r=Path::new(root);
    if let Ok(s)=std::fs::read_to_string(r.join("package.json")){if let Ok(v)=serde_json::from_str::<Value>(&s){if let Some(n)=v.get("name").and_then(Value::as_str){return Some(n.into())}}}
    if let Ok(s)=std::fs::read_to_string(r.join("go.mod")){for l in s.lines(){if let Some(v)=l.trim().strip_prefix("module "){return Some(v.trim().into())}}}
    for file in ["Cargo.toml","pyproject.toml"]{if let Ok(s)=std::fs::read_to_string(r.join(file)){for l in s.lines(){let l=l.trim();if let Some(v)=l.strip_prefix("name").and_then(|v|v.trim_start().strip_prefix('=')){let n=v.trim().trim_matches('"');if !n.is_empty(){return Some(n.into())}}}}}
    None
}
fn external_dependencies(analyses:&[FileAnalysis])->Vec<String>{
    let mut d=BTreeSet::new();for a in analyses{for i in &a.imports{if i.source.starts_with('.')||i.source.starts_with('/')||i.source.is_empty(){continue}let n=if let Some(s)=i.source.strip_prefix('@'){let mut p=s.split('/');match(p.next(),p.next()){(Some(a),Some(b))=>format!("@{}/{}",a,b),_=>i.source.clone()}}else{i.source.split('/').next().unwrap_or(&i.source).to_string()};d.insert(n);}}d.into_iter().collect()
}

async fn build_graph(path:&str)->anyhow::Result<(DependencyGraph,ScanReport)>{
    let started=std::time::Instant::now();let parser=LanguageParser::new();let files=discover(path)?;let mut analyses=Vec::new();
    for p in files{let s=p.to_string_lossy().to_string();if let Ok(a)=parser.parse_file(&s).await{analyses.push(a)}}
    let mut graph=DependencyGraph::new();for a in &analyses{graph.add_file(a)?}graph.build_call_graph()?;graph.build_type_graph()?;
    let analyzer=ArchitectureAnalyzer::new();let patterns=analyzer.detect_patterns(&graph)?;let drift=analyzer.detect_drift(&graph,&patterns)?;
    let report=ScanReport{files_processed:analyses.len(),nodes:graph.node_count(),edges:graph.edge_count(),patterns,drift,snapshot_id:uuid::Uuid::new_v4().to_string(),duration_ms:started.elapsed().as_secs_f64()*1000.0,package_identity:package_identity(path),external_dependencies:external_dependencies(&analyses)};
    Ok((graph,report))
}

async fn health()->Json<Value>{Json(json!({"status":"healthy","service":"ckb-reality-server-v2","realityApi":"v2","graphPersistence":"session-memory","evidencePolicy":"static-runtime-predicted-separated"}))}
async fn scan(State(state):State<AppState>,Json(req):Json<ScanRequest>)->Result<Json<Value>,(StatusCode,String)>{
    let s=session(&state,session_key(req.repo_name.as_deref(),req.project_id.as_deref())).await;let (graph,report)=build_graph(&req.path).await.map_err(internal)?;
    *s.graph.write().await=Some(graph);*s.report.write().await=Some(report.clone());*s.repo_path.write().await=Some(req.path);s.runtime_nodes.write().await.clear();s.runtime_edges.write().await.clear();
    Ok(Json(json!({"status":"success","filesProcessed":report.files_processed,"nodes":report.nodes,"edges":report.edges,"violationsFound":report.drift.len(),"snapshotId":report.snapshot_id})))
}
async fn report(State(state):State<AppState>,Query(q):Query<HashMap<String,String>>)->Result<Json<ScanReport>,(StatusCode,String)>{let s=session(&state,session_key(q.get("repo").map(String::as_str),q.get("project_id").map(String::as_str))).await;s.report.read().await.clone().map(Json).ok_or((StatusCode::NOT_FOUND,"No scan has been run for this session".into()))}
fn ct(v:Option<&str>)->ChangeType{match v.unwrap_or("modify").to_ascii_lowercase().as_str(){"add"=>ChangeType::Add,"delete"=>ChangeType::Delete,"rename"=>ChangeType::Rename,_=>ChangeType::Modify}}
async fn impact(State(state):State<AppState>,Json(req):Json<ImpactRequest>)->Result<Json<Value>,(StatusCode,String)>{
    let s=session(&state,session_key(req.repo_name.as_deref(),req.project_id.as_deref())).await;
    if s.graph.read().await.is_none(){let path=req.path.as_deref().ok_or((StatusCode::PRECONDITION_REQUIRED,"Scan first or provide path".into()))?;let (g,r)=build_graph(path).await.map_err(internal)?;*s.graph.write().await=Some(g);*s.report.write().await=Some(r);*s.repo_path.write().await=Some(path.into())}
    let g=s.graph.read().await;let g=g.as_ref().ok_or((StatusCode::PRECONDITION_REQUIRED,"No graph".into()))?;let affected=g.find_affected_nodes(&req.file,req.line).map_err(internal)?;let result=g.calculate_impact(&affected,ct(req.change_type.as_deref())).map_err(internal)?;
    Ok(Json(json!({"kind":"predicted","confidencePolicy":"derived-per-path","assumptions":["Current scanned graph is the baseline"],"evidence":[{"source":"ast-graph","ref":format!("{}:{}",req.file,req.line)}],"result":result})))
}

fn kind<T:std::fmt::Debug>(v:T)->String{format!("{:?}",v).to_ascii_lowercase()}
fn runtime_for(runtime:&HashMap<NodeId,RuntimeMetrics>,n:&Node)->Option<RuntimeMetrics>{if let Some(m)=runtime.get(&n.id){return Some(m.clone())}let p=n.path.to_string_lossy().replace('\\',"/");runtime.iter().find_map(|(id,m)|{let raw=id.0.replace('\\',"/");if raw==n.name||raw.ends_with(&format!("::{}",n.name))||(raw.starts_with(&format!("{}::",p))&&raw.ends_with(&n.name)){Some(m.clone())}else{None}})}

async fn graph_api(State(state):State<AppState>,Query(q):Query<HashMap<String,String>>)->Result<Json<Value>,(StatusCode,String)>{
    let s=session(&state,session_key(q.get("repo").map(String::as_str),q.get("project_id").map(String::as_str))).await;let g=s.graph.read().await;let g=g.as_ref().ok_or((StatusCode::PRECONDITION_REQUIRED,"No scan has been run for this session".into()))?;let rn=s.runtime_nodes.read().await.clone();let re=s.runtime_edges.read().await.clone();
    let nodes=g.nodes().into_iter().map(|n|{let r=runtime_for(&rn,n);json!({"id":n.id.0,"name":n.name,"kind":kind(n.kind),"path":n.path,"line":n.line,"column":n.column,"metadata":n.metadata,"runtime":r.as_ref().map(|m|json!({"invocationCount":m.execution_count,"avgLatencyMs":m.avg_latency_ms,"errorRate":m.error_rate,"isHotpath":m.is_hotpath})),"intelligence":{"kind":if r.is_some(){"runtime"}else{"static"},"confidence":1.0,"evidence":[{"source":"tree-sitter-ast","ref":format!("{}:{}:{}",n.path.to_string_lossy(),n.line,n.column)}],"explanation":if r.is_some(){"Source symbol with observed telemetry overlay."}else{"Source symbol discovered from AST analysis."}}})}).collect::<Vec<_>>();
    let links=g.edges().into_iter().map(|e|{let k=format!("{}->{}",e.from.0,e.to.0);let r=re.get(&k);json!({"id":e.id,"source":e.from.0,"target":e.to.0,"kind":kind(e.kind),"weight":e.weight,"metadata":e.metadata,"runtime":r.map(|x|json!({"invocationCount":x.invocation_count,"avgLatencyMs":x.avg_latency_ms(),"errorRate":x.error_rate(),"lastSeenUnixNano":x.last_seen_unix_nano,"traceId":x.trace_id})),"intelligence":{"kind":if r.is_some(){"runtime"}else{"static"},"confidence":1.0,"evidence":[{"source":if r.is_some(){"otlp+ast"}else{"ast-graph"},"ref":k}],"explanation":if r.is_some(){"Static relationship confirmed by observed OTLP parent-child execution."}else{"Structural relationship discovered from source analysis."}}})}).collect::<Vec<_>>();
    let snapshot=s.report.read().await.as_ref().map(|r|r.snapshot_id.clone()).unwrap_or_default();Ok(Json(json!({"graph":{"nodes":nodes,"links":links},"snapshotId":snapshot,"generatedAt":chrono::Utc::now().to_rfc3339()})))
}

fn scalar(v:&Value)->Option<String>{v.as_str().map(str::to_string).or_else(||v.get("stringValue").and_then(Value::as_str).map(str::to_string)).or_else(||v.get("intValue").and_then(|x|x.as_str().map(str::to_string).or_else(||x.as_u64().map(|n|n.to_string()))))}
fn attrs(v:Option<&Value>)->HashMap<String,String>{let mut o=HashMap::new();if let Some(a)=v.and_then(Value::as_array){for i in a{if let(Some(k),Some(v))=(i.get("key").and_then(Value::as_str),i.get("value").and_then(scalar)){o.insert(k.into(),v)}}}else if let Some(m)=v.and_then(Value::as_object){for(k,v)in m{if let Some(v)=scalar(v){o.insert(k.clone(),v)}}}o}
fn u64v(v:Option<&Value>)->u64{v.and_then(Value::as_u64).or_else(||v.and_then(Value::as_str).and_then(|s|s.parse().ok())).unwrap_or(0)}
fn spans(root:&Value)->Vec<Value>{if let Some(a)=root.as_array(){return a.clone()}let mut out=Vec::new();if let Some(rs)=root.get("resourceSpans").and_then(Value::as_array){for r in rs{let ra=attrs(r.get("resource").and_then(|x|x.get("attributes")));if let Some(ss)=r.get("scopeSpans").or_else(||r.get("instrumentationLibrarySpans")).and_then(Value::as_array){for s in ss{if let Some(xs)=s.get("spans").and_then(Value::as_array){for x in xs{let mut y=x.clone();if let Some(m)=y.as_object_mut(){let mut a=ra.clone();a.extend(attrs(x.get("attributes")));m.insert("_ckbMergedAttributes".into(),serde_json::to_value(a).unwrap_or(Value::Null))}out.push(y)}}}}}}out}
fn canonical(s:&Value)->String{let a:HashMap<String,String>=s.get("_ckbMergedAttributes").and_then(|v|serde_json::from_value(v.clone()).ok()).unwrap_or_else(||attrs(s.get("attributes")));let f=a.get("code.file.path").or_else(||a.get("code.filepath")).or_else(||a.get("code.file.name"));let fun=a.get("code.function.name").or_else(||a.get("code.function")).or_else(||a.get("function.name"));let ns=a.get("code.namespace").or_else(||a.get("service.name"));match(f,fun,ns){(Some(f),Some(fun),_)=>format!("{}::{}",f.replace('\\',"/"),fun),(Some(f),None,_)=>format!("{}::file",f.replace('\\',"/")),(None,Some(fun),Some(ns))=>format!("{}::{}",ns,fun),(None,Some(fun),None)=>fun.clone(),(None,None,Some(ns))=>format!("{}::{}",ns,s.get("name").and_then(Value::as_str).unwrap_or("span")),_=>s.get("name").and_then(Value::as_str).unwrap_or("span").into()}}
fn errspan(s:&Value)->bool{let c=s.get("status").and_then(|v|v.get("code"));c.and_then(Value::as_u64).map(|n|n==2).or_else(||c.and_then(Value::as_str).map(|s|matches!(s.to_ascii_uppercase().as_str(),"2"|"ERROR"|"STATUS_CODE_ERROR"))).unwrap_or(false)}
fn edge_observations(raw:&str)->anyhow::Result<HashMap<String,RuntimeEdgeObservation>>{let root:Value=serde_json::from_str(raw)?;let ss=spans(&root);let mut ids=HashMap::new();for s in &ss{let id=s.get("spanId").or_else(||s.get("span_id")).and_then(Value::as_str).unwrap_or("").to_string();let tr=s.get("traceId").or_else(||s.get("trace_id")).and_then(Value::as_str).unwrap_or("").to_string();if !id.is_empty(){ids.insert(id,(tr,canonical(s)))}}let mut out=HashMap::new();for s in &ss{let p=s.get("parentSpanId").or_else(||s.get("parent_span_id")).and_then(Value::as_str).unwrap_or("");let Some((pt,src))=ids.get(p).cloned()else{continue};let dst=canonical(s);let tr=s.get("traceId").or_else(||s.get("trace_id")).and_then(Value::as_str).unwrap_or(&pt).to_string();let st=u64v(s.get("startTimeUnixNano").or_else(||s.get("start_time_unix_nano")));let en=u64v(s.get("endTimeUnixNano").or_else(||s.get("end_time_unix_nano")));let key=format!("{}->{}",src,dst);let e=out.entry(key).or_insert(RuntimeEdgeObservation{source:src,target:dst,trace_id:tr,invocation_count:0,error_count:0,total_latency_ms:0.0,last_seen_unix_nano:0});e.invocation_count+=1;e.total_latency_ms+=en.saturating_sub(st)as f64/1_000_000.0;e.last_seen_unix_nano=e.last_seen_unix_nano.max(en);if errspan(s){e.error_count+=1}}Ok(out)}
fn merge_nodes(t:&mut HashMap<NodeId,RuntimeMetrics>,inc:HashMap<NodeId,RuntimeMetrics>){for(id,m)in inc{let e=t.entry(id).or_insert(RuntimeMetrics{execution_count:0,avg_latency_ms:0.0,error_rate:0.0,is_hotpath:false});let old=e.execution_count;let total=old.saturating_add(m.execution_count);if total>0{e.avg_latency_ms=((e.avg_latency_ms as f64*old as f64+m.avg_latency_ms as f64*m.execution_count as f64)/total as f64)as f32;e.error_rate=((e.error_rate as f64*old as f64+m.error_rate as f64*m.execution_count as f64)/total as f64)as f32}e.execution_count=total;e.is_hotpath=e.is_hotpath||m.is_hotpath||total>500}}
fn merge_edges(t:&mut HashMap<String,RuntimeEdgeObservation>,inc:HashMap<String,RuntimeEdgeObservation>){for(k,m)in inc{if let Some(e)=t.get_mut(&k){e.invocation_count+=m.invocation_count;e.error_count+=m.error_count;e.total_latency_ms+=m.total_latency_ms;e.last_seen_unix_nano=e.last_seen_unix_nano.max(m.last_seen_unix_nano);e.trace_id=m.trace_id}else{t.insert(k,m);}}}
async fn otlp(State(state):State<AppState>,Json(req):Json<OtlpRequest>)->Result<Json<Value>,(StatusCode,String)>{let s=session(&state,session_key(req.repo_name.as_deref(),req.project_id.as_deref())).await;let raw=if let Some(v)=req.raw_spans.or(req.otlp_json){v}else if let Some(v)=req.payload{serde_json::to_string(&v).map_err(internal)?}else{return Err((StatusCode::BAD_REQUEST,"Provide raw_spans, otlp_json, or payload".into()))};let n=OtlpReceiver::ingest_spans(&raw).map_err(internal)?;let e=edge_observations(&raw).map_err(internal)?;let summary=OtlpReceiver::summarize(&n);merge_nodes(&mut s.runtime_nodes.write().await,n);merge_edges(&mut s.runtime_edges.write().await,e);Ok(Json(json!({"status":"observed","kind":"runtime","spansIngested":summary.spans_ingested,"nodesUpdated":summary.nodes_updated,"errorSpans":summary.error_spans,"hotpathNodes":summary.hotpath_nodes,"runtimeEdges":s.runtime_edges.read().await.len(),"evidence":[{"source":"otlp","ref":"ingested-payload"}]})))}

async fn runtime(State(state):State<AppState>,Query(q):Query<HashMap<String,String>>)->Json<Value>{let s=session(&state,session_key(q.get("repo").map(String::as_str),q.get("project_id").map(String::as_str))).await;let n=s.runtime_nodes.read().await.iter().map(|(id,m)|json!({"id":id.0,"invocationCount":m.execution_count,"avgLatencyMs":m.avg_latency_ms,"errorRate":m.error_rate,"isHotpath":m.is_hotpath,"kind":"runtime","evidence":[{"source":"otlp","ref":id.0}]})).collect::<Vec<_>>();let e=s.runtime_edges.read().await.values().map(|r|json!({"source":r.source,"target":r.target,"traceId":r.trace_id,"invocationCount":r.invocation_count,"avgLatencyMs":r.avg_latency_ms(),"errorRate":r.error_rate(),"lastSeenUnixNano":r.last_seen_unix_nano,"kind":"runtime","evidence":[{"source":"otlp-parent-child","ref":format!("{}->{}",r.source,r.target)}]})).collect::<Vec<_>>();Json(json!({"observed":!n.is_empty()||!e.is_empty(),"nodes":n,"edges":e}))}
async fn traces(State(state):State<AppState>,Query(q):Query<HashMap<String,String>>)->Json<Value>{let s=session(&state,session_key(q.get("repo").map(String::as_str),q.get("project_id").map(String::as_str))).await;let e=s.runtime_edges.read().await;let mut t:HashMap<String,Vec<Value>>=HashMap::new();for x in e.values(){t.entry(x.trace_id.clone()).or_default().push(json!({"source":x.source,"target":x.target,"invocationCount":x.invocation_count,"avgLatencyMs":x.avg_latency_ms(),"errorRate":x.error_rate(),"lastSeenUnixNano":x.last_seen_unix_nano}))}Json(json!({"kind":"runtime","observed":!e.is_empty(),"traces":t}))}
async fn source(State(state):State<AppState>,Query(q):Query<HashMap<String,String>>)->Result<Json<Value>,(StatusCode,String)>{let s=session(&state,session_key(q.get("repo").map(String::as_str),q.get("project_id").map(String::as_str))).await;let id=q.get("node_id").ok_or((StatusCode::BAD_REQUEST,"node_id is required".into()))?;let g=s.graph.read().await;let g=g.as_ref().ok_or((StatusCode::PRECONDITION_REQUIRED,"No scan".into()))?;let n=g.nodes().into_iter().find(|n|n.id.0==*id).ok_or((StatusCode::NOT_FOUND,"Node not found".into()))?;Ok(Json(json!({"id":n.id.0,"name":n.name,"kind":kind(n.kind),"path":n.path,"line":n.line,"column":n.column,"span":{"startLine":n.metadata.get("start_line"),"startColumn":n.metadata.get("start_column"),"endLine":n.metadata.get("end_line"),"endColumn":n.metadata.get("end_column"),"byteStart":n.metadata.get("byte_start"),"byteEnd":n.metadata.get("byte_end")},"kindOfEvidence":"static","confidence":1.0,"evidence":[{"source":"tree-sitter-ast","ref":n.id.0}]})))}
async fn history(State(state):State<AppState>,Query(q):Query<HashMap<String,String>>)->Result<Json<Value>,(StatusCode,String)>{let s=session(&state,session_key(q.get("repo").map(String::as_str),q.get("project_id").map(String::as_str))).await;let p=s.repo_path.read().await.clone().or_else(||q.get("path").cloned()).ok_or((StatusCode::PRECONDITION_REQUIRED,"Scan first or provide path".into()))?;let max=q.get("max_commits").and_then(|v|v.parse::<usize>().ok()).unwrap_or(50).min(500);let t=GitDriftAnalyzer::build_timeline(&p,max).map_err(internal)?;Ok(Json(json!({"kind":"static","source":"git","timeline":t,"evidence":[{"source":"git-history","ref":p}]})))}
async fn test_gaps(State(state):State<AppState>,Query(q):Query<HashMap<String,String>>)->Result<Json<Value>,(StatusCode,String)>{let s=session(&state,session_key(q.get("repo").map(String::as_str),q.get("project_id").map(String::as_str))).await;let g=s.graph.read().await;let g=g.as_ref().ok_or((StatusCode::PRECONDITION_REQUIRED,"No scan".into()))?;Ok(Json(serde_json::to_value(TestCoverageAnalyzer::analyze_gaps(g).map_err(internal)?).map_err(internal)?))}
async fn rules(State(state):State<AppState>,Query(q):Query<HashMap<String,String>>)->Result<String,(StatusCode,String)>{let s=session(&state,session_key(q.get("repo").map(String::as_str),q.get("project_id").map(String::as_str))).await;let g=s.graph.read().await;let g=g.as_ref().ok_or((StatusCode::PRECONDITION_REQUIRED,"No scan".into()))?;ArchitectureAnalyzer::new().generate_ai_guidelines(g).map_err(internal)}

#[tokio::main]
async fn main()->anyhow::Result<()>{tracing_subscriber::fmt::init();let state=AppState{default_session:Session::new(),sessions:Arc::new(RwLock::new(HashMap::new())),api_key:std::env::var("CKB_API_KEY").ok().filter(|v|!v.is_empty()).map(Arc::new)};if state.api_key.is_none(){warn!("CKB_API_KEY is not configured; Reality API is unauthenticated")}
    let protected=Router::new().route("/api/v1/scan",post(scan)).route("/api/v1/report",get(report)).route("/api/v1/impact",post(impact)).route("/api/v1/otlp",post(otlp)).route("/api/v1/drift-timeline",get(history)).route("/api/v1/test-gaps",get(test_gaps)).route("/api/v1/rules",get(rules)).route("/api/v1/intelligence/graph",get(graph_api)).route("/api/v1/intelligence/source",get(source)).route("/api/v1/intelligence/runtime",get(runtime)).route("/api/v1/intelligence/traces",get(traces)).route("/api/v1/intelligence/impact",post(impact)).route("/api/v1/intelligence/telemetry/otlp",post(otlp)).route("/api/v1/intelligence/history",get(history)).route_layer(middleware::from_fn_with_state(state.clone(),auth));
    let cors=match std::env::var("CKB_ALLOWED_ORIGIN"){Ok(v)if v=="*"=>CorsLayer::new().allow_origin(Any).allow_methods(Any).allow_headers(Any),Ok(v)=>CorsLayer::new().allow_origin(v.parse::<axum::http::HeaderValue>()?).allow_methods(Any).allow_headers(Any),Err(_)=>CorsLayer::new()};let app=Router::new().route("/health",get(health)).merge(protected).layer(cors).with_state(state);let port=std::env::var("PORT").ok().and_then(|v|v.parse().ok()).unwrap_or(3000);let all=std::env::var("CKB_BIND_ALL").map(|v|v=="1"||v.eq_ignore_ascii_case("true")).unwrap_or(false);let host=if all{[0,0,0,0]}else{[127,0,0,1]};let addr=std::net::SocketAddr::from((host,port));info!("CKB Reality API v2 listening on {}",addr);let l=tokio::net::TcpListener::bind(addr).await?;axum::serve(l,app).await?;Ok(())}
