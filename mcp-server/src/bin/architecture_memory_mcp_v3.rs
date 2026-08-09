use ckb_core::{ArchitectureMemoryEngine, CausalArchitectureEngine, DependencyGraph, FileAnalysis, LanguageParser, NodeId};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::{BTreeSet, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::sync::RwLock;

#[derive(Default)]
struct MemorySession {
    graph: Option<DependencyGraph>,
    root: Option<String>,
    git_head: Option<String>,
    workspace_fingerprint: Option<String>,
    saved_at: Option<String>,
    last_delta: Option<Value>,
}

#[derive(Serialize, Deserialize)]
struct PersistedMemoryV3 {
    version: String,
    root: String,
    saved_at: String,
    git_head: Option<String>,
    workspace_fingerprint: Option<String>,
    graph: DependencyGraph,
}

// Backward-compatible reader for durable memories written by the previous MCP
// binary. A resumed v1 memory is upgraded to v3 the next time it is refreshed.
#[derive(Serialize, Deserialize)]
struct PersistedMemoryV1 {
    version: String,
    root: String,
    saved_at: String,
    git_head: Option<String>,
    graph: DependencyGraph,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ArchitectureDelta {
    added_nodes: Vec<String>,
    removed_nodes: Vec<String>,
    added_edges: Vec<String>,
    removed_edges: Vec<String>,
    node_delta: i64,
    edge_delta: i64,
    from_nodes: usize,
    to_nodes: usize,
    from_edges: usize,
    to_edges: usize,
    synthetic: bool,
}

fn supported(path: &Path) -> bool {
    matches!(
        path.extension().and_then(|v| v.to_str()).unwrap_or(""),
        "ts" | "tsx" | "js" | "jsx" | "mjs" | "py" | "go" | "rs" | "java"
    )
}

fn discover(root: &str) -> anyhow::Result<Vec<PathBuf>> {
    let mut out = Vec::new();
    let mut stack = vec![PathBuf::from(root)];
    while let Some(dir) = stack.pop() {
        for entry in std::fs::read_dir(&dir)? {
            let entry = entry?;
            let path = entry.path();
            let name = path.file_name().and_then(|v| v.to_str()).unwrap_or("");
            if path.is_dir() {
                if !matches!(name, ".git" | "node_modules" | "target" | "dist" | "build" | ".next" | "vendor" | "coverage" | ".turbo" | ".yarn") {
                    stack.push(path);
                }
            } else if supported(&path) {
                out.push(path);
            }
        }
    }
    out.sort();
    Ok(out)
}

async fn build_graph(path: &str) -> anyhow::Result<(DependencyGraph, usize)> {
    let parser = LanguageParser::new();
    let files = discover(path)?;
    let mut analyses: Vec<FileAnalysis> = Vec::new();
    for file in files {
        let file_path = file.to_string_lossy().to_string();
        if let Ok(analysis) = parser.parse_file(&file_path).await {
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
    Ok((graph, analyses.len()))
}

fn canonical_root(path: &str) -> String {
    std::fs::canonicalize(path)
        .unwrap_or_else(|_| PathBuf::from(path))
        .to_string_lossy()
        .replace('\\', "/")
}

fn run_git(path: &str, args: &[&str]) -> Option<String> {
    let output = std::process::Command::new("git")
        .arg("-C")
        .arg(path)
        .args(args)
        .output()
        .ok()?;
    if !output.status.success() { return None; }
    Some(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

fn git_head(path: &str) -> Option<String> {
    run_git(path, &["rev-parse", "HEAD"]).filter(|v| !v.is_empty())
}

fn stable_key(input: &str) -> String {
    // FNV-1a 64-bit is used only as a deterministic change fingerprint/key,
    // never for credentials or cryptographic integrity.
    let mut hash: u64 = 0xcbf29ce484222325;
    for byte in input.as_bytes() {
        hash ^= *byte as u64;
        hash = hash.wrapping_mul(0x100000001b3);
    }
    format!("{:016x}", hash)
}

fn fallback_source_fingerprint(root: &str) -> Option<String> {
    let files = discover(root).ok()?;
    let mut material = String::new();
    for file in files {
        material.push_str(&file.to_string_lossy());
        if let Ok(meta) = std::fs::metadata(&file) {
            material.push_str(&format!("|{}|", meta.len()));
        }
        if let Ok(bytes) = std::fs::read(&file) {
            // Hash each source file independently so the retained material is
            // bounded to one small fingerprint per file.
            let text = String::from_utf8_lossy(&bytes);
            material.push_str(&stable_key(&text));
        }
        material.push('\n');
    }
    Some(format!("source:{}", stable_key(&material)))
}

fn workspace_fingerprint(root: &str) -> Option<String> {
    let head = git_head(root);
    let status = run_git(root, &["status", "--porcelain=v1", "--untracked-files=all"]);
    if let Some(status) = status {
        let mut rows = Vec::new();
        for line in status.lines() {
            if line.len() < 4 { continue; }
            let code = line.get(0..2).unwrap_or("??").trim().to_string();
            let raw_path = line.get(3..).unwrap_or("").trim();
            let path = raw_path.split(" -> ").last().unwrap_or(raw_path).trim_matches('"');
            let object_hash = run_git(root, &["hash-object", "--", path]).unwrap_or_else(|| "missing".into());
            rows.push(format!("{}|{}|{}", code, path.replace('\\', "/"), object_hash));
        }
        rows.sort();
        let material = format!("head={}\n{}", head.clone().unwrap_or_else(|| "none".into()), rows.join("\n"));
        return Some(format!("git:{}", stable_key(&material)));
    }
    fallback_source_fingerprint(root)
}

fn memory_base_dir() -> PathBuf {
    if let Ok(value) = std::env::var("CKB_MEMORY_DIR") {
        if !value.trim().is_empty() { return PathBuf::from(value); }
    }
    if let Ok(home) = std::env::var("HOME").or_else(|_| std::env::var("USERPROFILE")) {
        if !home.trim().is_empty() { return PathBuf::from(home).join(".ckb").join("memory"); }
    }
    PathBuf::from(".ckb-memory")
}

fn memory_path(root: &str) -> PathBuf {
    memory_base_dir().join(format!("{}.ckbmem", stable_key(&canonical_root(root))))
}

fn persist_memory(root: &str, graph: &DependencyGraph) -> anyhow::Result<PersistedMemoryV3> {
    let canonical = canonical_root(root);
    let persisted = PersistedMemoryV3 {
        version: "ckb-durable-memory-v3".into(),
        root: canonical.clone(),
        saved_at: chrono::Utc::now().to_rfc3339(),
        git_head: git_head(&canonical),
        workspace_fingerprint: workspace_fingerprint(&canonical),
        graph: bincode::deserialize(&bincode::serialize(graph)?)?,
    };
    let path = memory_path(&canonical);
    if let Some(parent) = path.parent() { std::fs::create_dir_all(parent)?; }
    let temp = path.with_extension("ckbmem.tmp");
    std::fs::write(&temp, bincode::serialize(&persisted)?)?;
    std::fs::rename(&temp, &path)?;
    Ok(persisted)
}

fn load_memory(root: &str) -> anyhow::Result<PersistedMemoryV3> {
    let path = memory_path(root);
    let bytes = std::fs::read(&path)
        .map_err(|e| anyhow::anyhow!("No durable CKB memory found at {}: {}", path.display(), e))?;

    if let Ok(v3) = bincode::deserialize::<PersistedMemoryV3>(&bytes) {
        if v3.version == "ckb-durable-memory-v3" { return Ok(v3); }
    }
    if let Ok(v1) = bincode::deserialize::<PersistedMemoryV1>(&bytes) {
        if v1.version == "ckb-durable-memory-v1" {
            return Ok(PersistedMemoryV3 {
                version: "ckb-durable-memory-v1-upgraded-in-memory".into(),
                workspace_fingerprint: None,
                root: v1.root,
                saved_at: v1.saved_at,
                git_head: v1.git_head,
                graph: v1.graph,
            });
        }
    }
    anyhow::bail!("Unsupported or corrupt CKB architecture memory at {}", path.display())
}

fn freshness(root: &str, saved_head: Option<&str>, saved_workspace: Option<&str>) -> (String, Option<String>, Option<String>) {
    let current_head = git_head(root);
    let current_workspace = workspace_fingerprint(root);
    let state = match (saved_head, current_head.as_deref(), saved_workspace, current_workspace.as_deref()) {
        (Some(saved), Some(now), _, _) if saved != now => "commit-changed",
        (_, _, Some(saved), Some(now)) if saved != now => "worktree-changed",
        (Some(saved), Some(now), Some(saved_ws), Some(now_ws)) if saved == now && saved_ws == now_ws => "fresh",
        (Some(saved), Some(now), None, _) if saved == now => "legacy-head-only",
        _ => "unknown",
    };
    (state.into(), current_head, current_workspace)
}

fn edge_identity(edge: &ckb_core::Edge) -> String {
    format!("{}->{}/{:?}", edge.from.0, edge.to.0, edge.kind).to_ascii_lowercase()
}

fn architecture_delta(before: &DependencyGraph, after: &DependencyGraph) -> ArchitectureDelta {
    let old_nodes: HashSet<String> = before.nodes().into_iter().map(|n| n.id.0.clone()).collect();
    let new_nodes: HashSet<String> = after.nodes().into_iter().map(|n| n.id.0.clone()).collect();
    let old_edges: HashSet<String> = before.edges().into_iter().map(edge_identity).collect();
    let new_edges: HashSet<String> = after.edges().into_iter().map(edge_identity).collect();

    let mut added_nodes = new_nodes.difference(&old_nodes).cloned().collect::<Vec<_>>();
    let mut removed_nodes = old_nodes.difference(&new_nodes).cloned().collect::<Vec<_>>();
    let mut added_edges = new_edges.difference(&old_edges).cloned().collect::<Vec<_>>();
    let mut removed_edges = old_edges.difference(&new_edges).cloned().collect::<Vec<_>>();
    added_nodes.sort(); removed_nodes.sort(); added_edges.sort(); removed_edges.sort();

    ArchitectureDelta {
        node_delta: new_nodes.len() as i64 - old_nodes.len() as i64,
        edge_delta: new_edges.len() as i64 - old_edges.len() as i64,
        from_nodes: old_nodes.len(), to_nodes: new_nodes.len(),
        from_edges: old_edges.len(), to_edges: new_edges.len(),
        added_nodes, removed_nodes, added_edges, removed_edges,
        synthetic: false,
    }
}

fn tool_list() -> Value {
    json!({
        "tools": [
            {"name":"ckb_memory_scan","description":"Build and durably persist evidence-backed architecture memory for a repository.","inputSchema":{"type":"object","properties":{"path":{"type":"string"}},"required":["path"]}},
            {"name":"ckb_memory_resume","description":"Resume durable architecture memory across model/MCP restarts and verify Git + working-tree freshness.","inputSchema":{"type":"object","properties":{"path":{"type":"string"}},"required":["path"]}},
            {"name":"ckb_memory_refresh","description":"Refresh loaded memory from current source, persist it, and return the exact architecture delta since the prior memory state.","inputSchema":{"type":"object","properties":{"force":{"type":"boolean","default":false}}}},
            {"name":"ckb_memory_status","description":"Report loaded memory, persistence, Git HEAD and exact working-tree freshness.","inputSchema":{"type":"object","properties":{}}},
            {"name":"ckb_memory_delta","description":"Return the most recent real architecture delta produced by ckb_memory_refresh.","inputSchema":{"type":"object","properties":{}}},
            {"name":"ckb_memory_query","description":"Retrieve a bounded evidence-backed architecture neighborhood for a question/symbol/path.","inputSchema":{"type":"object","properties":{"query":{"type":"string"},"depth":{"type":"integer","minimum":0,"maximum":5,"default":2},"limit":{"type":"integer","minimum":1,"maximum":100,"default":12}},"required":["query"]}},
            {"name":"ckb_symbol_memory","description":"Retrieve bounded software memory centered on one symbol/path before editing it.","inputSchema":{"type":"object","properties":{"symbol":{"type":"string"},"depth":{"type":"integer","minimum":0,"maximum":5,"default":2},"limit":{"type":"integer","minimum":1,"maximum":100,"default":8}},"required":["symbol"]}},
            {"name":"ckb_context_capsule","description":"Create a compact model-ready architecture context capsule with freshness evidence and a strict character budget.","inputSchema":{"type":"object","properties":{"query":{"type":"string"},"depth":{"type":"integer","minimum":0,"maximum":5,"default":2},"limit":{"type":"integer","minimum":1,"maximum":100,"default":12},"max_chars":{"type":"integer","minimum":1000,"maximum":60000,"default":12000}},"required":["query"]}},
            {"name":"ckb_code_dna","description":"Return explainable Code DNA health/risk metrics from graph topology/cycles/runtime observations.","inputSchema":{"type":"object","properties":{}}},
            {"name":"ckb_causal_path","description":"Explain the shortest proven directed architecture path between exact CKB node IDs.","inputSchema":{"type":"object","properties":{"source":{"type":"string"},"target":{"type":"string"},"max_depth":{"type":"integer","minimum":1,"maximum":32,"default":12}},"required":["source","target"]}},
            {"name":"ckb_failure_cone","description":"Return the real transitive upstream dependent cone for a symbol without pretending a runtime failure occurred.","inputSchema":{"type":"object","properties":{"root":{"type":"string"},"max_depth":{"type":"integer","minimum":1,"maximum":32,"default":12}},"required":["root"]}}
        ]
    })
}

fn text_result(value: Value) -> Value {
    json!({"content":[{"type":"text","text":serde_json::to_string_pretty(&value).unwrap_or_else(|_|"{}".into())}],"isError":false})
}
fn error_result(message: impl Into<String>) -> Value {
    json!({"content":[{"type":"text","text":message.into()}],"isError":true})
}

async fn call_tool(session: &Arc<RwLock<MemorySession>>, name: &str, args: &Value) -> Value {
    match name {
        "ckb_memory_scan" => {
            let Some(path) = args.get("path").and_then(Value::as_str) else { return error_result("path is required"); };
            match build_graph(path).await {
                Ok((graph, files)) => match persist_memory(path, &graph) {
                    Ok(persisted) => {
                        let response = json!({"status":"remembered","path":persisted.root,"filesProcessed":files,"nodes":graph.node_count(),"edges":graph.edge_count(),"savedAt":persisted.saved_at,"gitHead":persisted.git_head,"workspaceFingerprint":persisted.workspace_fingerprint,"memoryFile":memory_path(path).to_string_lossy(),"freshness":"fresh","memoryLifetime":"durable across MCP/model restarts","synthetic":false});
                        let mut state = session.write().await;
                        state.graph = Some(graph); state.root = Some(persisted.root); state.git_head = persisted.git_head; state.workspace_fingerprint = persisted.workspace_fingerprint; state.saved_at = Some(persisted.saved_at); state.last_delta = None;
                        text_result(response)
                    }
                    Err(e) => error_result(format!("Architecture scan succeeded but durable persistence failed: {e}")),
                },
                Err(e) => error_result(format!("CKB memory scan failed: {e}")),
            }
        }
        "ckb_memory_resume" => {
            let Some(path) = args.get("path").and_then(Value::as_str) else { return error_result("path is required"); };
            match load_memory(path) {
                Ok(persisted) => {
                    let (fresh, current_head, current_workspace) = freshness(&persisted.root, persisted.git_head.as_deref(), persisted.workspace_fingerprint.as_deref());
                    let response = json!({"status":"resumed","path":persisted.root,"nodes":persisted.graph.node_count(),"edges":persisted.graph.edge_count(),"savedAt":persisted.saved_at,"savedGitHead":persisted.git_head,"currentGitHead":current_head,"savedWorkspaceFingerprint":persisted.workspace_fingerprint,"currentWorkspaceFingerprint":current_workspace,"freshness":fresh,"mustRefreshBeforeHighConfidenceEdits":fresh != "fresh","memoryFile":memory_path(path).to_string_lossy(),"synthetic":false});
                    let mut state = session.write().await;
                    state.graph = Some(persisted.graph); state.root = Some(persisted.root); state.git_head = persisted.git_head; state.workspace_fingerprint = persisted.workspace_fingerprint; state.saved_at = Some(persisted.saved_at); state.last_delta = None;
                    text_result(response)
                }
                Err(e) => error_result(format!("CKB durable memory resume failed: {e}")),
            }
        }
        "ckb_memory_refresh" => {
            let force = args.get("force").and_then(Value::as_bool).unwrap_or(false);
            let (root, old_graph, saved_head, saved_workspace) = {
                let state = session.read().await;
                let Some(root) = state.root.clone() else { return error_result("No architecture memory loaded. Resume or scan first."); };
                let Some(graph) = state.graph.clone() else { return error_result("No architecture graph loaded."); };
                (root, graph, state.git_head.clone(), state.workspace_fingerprint.clone())
            };
            let (fresh, _, _) = freshness(&root, saved_head.as_deref(), saved_workspace.as_deref());
            if fresh == "fresh" && !force {
                return text_result(json!({"status":"already-fresh","path":root,"nodes":old_graph.node_count(),"edges":old_graph.edge_count(),"synthetic":false}));
            }
            match build_graph(&root).await {
                Ok((new_graph, files)) => {
                    let delta = architecture_delta(&old_graph, &new_graph);
                    match persist_memory(&root, &new_graph) {
                        Ok(persisted) => {
                            let delta_value = serde_json::to_value(&delta).unwrap_or(Value::Null);
                            let response = json!({"status":"refreshed","path":persisted.root,"filesProcessed":files,"savedAt":persisted.saved_at,"gitHead":persisted.git_head,"workspaceFingerprint":persisted.workspace_fingerprint,"delta":delta_value,"synthetic":false});
                            let mut state = session.write().await;
                            state.graph = Some(new_graph); state.root = Some(persisted.root); state.git_head = persisted.git_head; state.workspace_fingerprint = persisted.workspace_fingerprint; state.saved_at = Some(persisted.saved_at); state.last_delta = Some(delta_value);
                            text_result(response)
                        }
                        Err(e) => error_result(format!("Refreshed graph but durable persistence failed: {e}")),
                    }
                }
                Err(e) => error_result(format!("CKB memory refresh failed: {e}")),
            }
        }
        "ckb_memory_status" => {
            let state = session.read().await;
            let Some(graph) = state.graph.as_ref() else { return text_result(json!({"loaded":false,"message":"No architecture memory is loaded. Use ckb_memory_resume or ckb_memory_scan.","memoryDirectory":memory_base_dir().to_string_lossy(),"synthetic":false})); };
            let root = state.root.as_deref().unwrap_or("");
            let (fresh, current_head, current_workspace) = freshness(root, state.git_head.as_deref(), state.workspace_fingerprint.as_deref());
            text_result(json!({"loaded":true,"path":root,"nodes":graph.node_count(),"edges":graph.edge_count(),"savedAt":state.saved_at,"savedGitHead":state.git_head,"currentGitHead":current_head,"savedWorkspaceFingerprint":state.workspace_fingerprint,"currentWorkspaceFingerprint":current_workspace,"freshness":fresh,"mustRefreshBeforeHighConfidenceEdits":fresh != "fresh","memoryFile":memory_path(root).to_string_lossy(),"synthetic":false}))
        }
        "ckb_memory_delta" => {
            let state = session.read().await;
            text_result(state.last_delta.clone().unwrap_or_else(|| json!({"available":false,"message":"No refresh delta exists in this MCP session yet.","synthetic":false})))
        }
        "ckb_memory_query" | "ckb_symbol_memory" | "ckb_context_capsule" => {
            let state = session.read().await;
            let Some(graph) = state.graph.as_ref() else { return error_result("No architecture memory loaded. Resume or scan first."); };
            let query_key = if name == "ckb_symbol_memory" { "symbol" } else { "query" };
            let query = args.get(query_key).and_then(Value::as_str).unwrap_or("");
            if query.is_empty() { return error_result(format!("{query_key} is required")); }
            let depth = args.get("depth").and_then(Value::as_u64).unwrap_or(2) as usize;
            let limit = args.get("limit").and_then(Value::as_u64).unwrap_or(if name == "ckb_symbol_memory" { 8 } else { 12 }) as usize;
            match ArchitectureMemoryEngine::query(graph, query, depth, limit) {
                Ok(slice) if name == "ckb_context_capsule" => {
                    let max_chars = args.get("max_chars").and_then(Value::as_u64).unwrap_or(12_000).clamp(1_000, 60_000) as usize;
                    let root = state.root.as_deref().unwrap_or("");
                    let (fresh, current_head, current_workspace) = freshness(root, state.git_head.as_deref(), state.workspace_fingerprint.as_deref());
                    let mut context = slice.context.clone();
                    if context.len() > max_chars {
                        let mut boundary = max_chars.min(context.len());
                        while boundary > 0 && !context.is_char_boundary(boundary) { boundary -= 1; }
                        context.truncate(boundary);
                        context.push_str("\n[CKB capsule truncated to requested character budget]\n");
                    }
                    text_result(json!({"version":"ckb-context-capsule-v1","query":query,"freshness":fresh,"currentGitHead":current_head,"currentWorkspaceFingerprint":current_workspace,"mustRefreshBeforeHighConfidenceEdits":fresh != "fresh","rootIds":slice.root_ids,"nodes":slice.nodes,"edges":slice.edges,"context":context,"characterBudget":max_chars,"evidencePolicy":slice.evidence_policy,"synthetic":false}))
                }
                Ok(slice) => text_result(serde_json::to_value(slice).unwrap_or(Value::Null)),
                Err(e) => error_result(format!("Architecture memory retrieval failed: {e}")),
            }
        }
        "ckb_code_dna" => {
            let state = session.read().await;
            let Some(graph) = state.graph.as_ref() else { return error_result("No architecture memory loaded. Resume or scan first."); };
            match ArchitectureMemoryEngine::code_dna(graph) { Ok(report) => text_result(serde_json::to_value(report).unwrap_or(Value::Null)), Err(e) => error_result(format!("Code DNA analysis failed: {e}")) }
        }
        "ckb_causal_path" => {
            let state = session.read().await;
            let Some(graph) = state.graph.as_ref() else { return error_result("No architecture memory loaded. Resume or scan first."); };
            let Some(source) = args.get("source").and_then(Value::as_str) else { return error_result("source is required"); };
            let Some(target) = args.get("target").and_then(Value::as_str) else { return error_result("target is required"); };
            let max_depth = args.get("max_depth").and_then(Value::as_u64).unwrap_or(12) as usize;
            match CausalArchitectureEngine::shortest_path(graph, &NodeId(source.into()), &NodeId(target.into()), max_depth) { Ok(report) => text_result(serde_json::to_value(report).unwrap_or(Value::Null)), Err(e) => error_result(format!("Causal path analysis failed: {e}")) }
        }
        "ckb_failure_cone" => {
            let state = session.read().await;
            let Some(graph) = state.graph.as_ref() else { return error_result("No architecture memory loaded. Resume or scan first."); };
            let Some(root) = args.get("root").and_then(Value::as_str) else { return error_result("root is required"); };
            let max_depth = args.get("max_depth").and_then(Value::as_u64).unwrap_or(12) as usize;
            match CausalArchitectureEngine::failure_cone(graph, &NodeId(root.into()), max_depth) { Ok(report) => text_result(serde_json::to_value(report).unwrap_or(Value::Null)), Err(e) => error_result(format!("Failure-cone analysis failed: {e}")) }
        }
        _ => error_result(format!("Unknown tool: {name}")),
    }
}

async fn handle(session: &Arc<RwLock<MemorySession>>, request: Value) -> Value {
    let id = request.get("id").cloned().unwrap_or(Value::Null);
    let method = request.get("method").and_then(Value::as_str).unwrap_or("");
    let result = match method {
        "initialize" => json!({
            "protocolVersion":"2024-11-05",
            "capabilities":{"tools":{}},
            "serverInfo":{"name":"ckb-architecture-memory","version":"3.0.0"},
            "instructions":"Resume existing CKB memory first. Check freshness before edits; worktree changes and new commits are both detected. Refresh stale memory to receive an exact architecture delta, then use bounded queries/context capsules, causal paths, failure cones and Code DNA so the model reasons from durable software evidence rather than rediscovering the repository."
        }),
        "notifications/initialized" => return Value::Null,
        "tools/list" => tool_list(),
        "tools/call" => {
            let params = request.get("params").cloned().unwrap_or_else(||json!({}));
            let name = params.get("name").and_then(Value::as_str).unwrap_or("");
            let args = params.get("arguments").cloned().unwrap_or_else(||json!({}));
            call_tool(session,name,&args).await
        }
        "ping" => json!({}),
        _ => return json!({"jsonrpc":"2.0","id":id,"error":{"code":-32601,"message":format!("Method not found: {method}")}}),
    };
    json!({"jsonrpc":"2.0","id":id,"result":result})
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let session = Arc::new(RwLock::new(MemorySession::default()));
    let stdin = tokio::io::stdin();
    let mut lines = BufReader::new(stdin).lines();
    let mut stdout = tokio::io::stdout();
    while let Some(line) = lines.next_line().await? {
        if line.trim().is_empty() { continue; }
        let response = match serde_json::from_str::<Value>(&line) {
            Ok(request) => handle(&session,request).await,
            Err(e) => json!({"jsonrpc":"2.0","id":Value::Null,"error":{"code":-32700,"message":format!("Parse error: {e}")}}),
        };
        if response.is_null() { continue; }
        stdout.write_all(serde_json::to_string(&response)?.as_bytes()).await?;
        stdout.write_all(b"\n").await?;
        stdout.flush().await?;
    }
    Ok(())
}
