use ckb_core::{ArchitectureMemoryEngine, CausalArchitectureEngine, DependencyGraph, FileAnalysis, LanguageParser, NodeId};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::sync::RwLock;

#[derive(Default)]
struct MemorySession {
    graph: Option<DependencyGraph>,
    root: Option<String>,
    git_head: Option<String>,
    saved_at: Option<String>,
}

#[derive(Serialize, Deserialize)]
struct PersistedMemory {
    version: String,
    root: String,
    saved_at: String,
    git_head: Option<String>,
    graph: DependencyGraph,
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
                if !matches!(name, ".git" | "node_modules" | "target" | "dist" | "build" | ".next" | "vendor") {
                    stack.push(path);
                }
            } else if supported(&path) {
                out.push(path);
            }
        }
    }
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

fn git_head(path: &str) -> Option<String> {
    let output = std::process::Command::new("git")
        .args(["-C", path, "rev-parse", "HEAD"])
        .output()
        .ok()?;
    if !output.status.success() { return None; }
    let head = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if head.is_empty() { None } else { Some(head) }
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

fn stable_key(input: &str) -> String {
    // FNV-1a 64-bit: deterministic and dependency-free. This is a filesystem
    // key, not a security hash; no source contents or credentials are encoded.
    let mut hash: u64 = 0xcbf29ce484222325;
    for byte in input.as_bytes() {
        hash ^= *byte as u64;
        hash = hash.wrapping_mul(0x100000001b3);
    }
    format!("{:016x}", hash)
}

fn memory_path(root: &str) -> PathBuf {
    memory_base_dir().join(format!("{}.ckbmem", stable_key(&canonical_root(root))))
}

fn persist_memory(root: &str, graph: &DependencyGraph) -> anyhow::Result<PersistedMemory> {
    let canonical = canonical_root(root);
    let persisted = PersistedMemory {
        version: "ckb-durable-memory-v1".into(),
        root: canonical.clone(),
        saved_at: chrono::Utc::now().to_rfc3339(),
        git_head: git_head(&canonical),
        graph: bincode::deserialize(&bincode::serialize(graph)?)?,
    };
    let path = memory_path(&canonical);
    if let Some(parent) = path.parent() { std::fs::create_dir_all(parent)?; }
    let temp = path.with_extension("ckbmem.tmp");
    std::fs::write(&temp, bincode::serialize(&persisted)?)?;
    std::fs::rename(&temp, &path)?;
    Ok(persisted)
}

fn load_memory(root: &str) -> anyhow::Result<PersistedMemory> {
    let path = memory_path(root);
    let bytes = std::fs::read(&path)
        .map_err(|e| anyhow::anyhow!("No durable CKB memory found at {}: {}", path.display(), e))?;
    let persisted: PersistedMemory = bincode::deserialize(&bytes)?;
    if persisted.version != "ckb-durable-memory-v1" {
        anyhow::bail!("Unsupported architecture memory version: {}", persisted.version);
    }
    Ok(persisted)
}

fn freshness(root: &str, saved_head: Option<&str>) -> (&'static str, Option<String>) {
    let current = git_head(root);
    match (saved_head, current.as_deref()) {
        (Some(saved), Some(now)) if saved == now => ("fresh", current),
        (Some(_), Some(_)) => ("stale", current),
        _ => ("unknown", current),
    }
}

fn tool_list() -> Value {
    json!({
        "tools": [
            {
                "name": "ckb_memory_scan",
                "description": "Scan a repository into architecture memory and durably persist the graph so later MCP processes/models can resume it without rediscovering the codebase.",
                "inputSchema": {
                    "type": "object",
                    "properties": { "path": { "type": "string", "description": "Local repository path" } },
                    "required": ["path"]
                }
            },
            {
                "name": "ckb_memory_resume",
                "description": "Resume the last durable architecture memory for a repository without rescanning. Returns freshness against the current Git HEAD when Git metadata is available.",
                "inputSchema": {
                    "type": "object",
                    "properties": { "path": { "type": "string", "description": "Local repository path used for the prior memory scan" } },
                    "required": ["path"]
                }
            },
            {
                "name": "ckb_memory_status",
                "description": "Report which repository is currently loaded in this MCP process, graph size, persistence location, save time and Git freshness.",
                "inputSchema": { "type": "object", "properties": {} }
            },
            {
                "name": "ckb_memory_query",
                "description": "Retrieve a bounded evidence-backed architecture neighborhood relevant to a natural-language, symbol, file-path, runtime, or change-impact question.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "query": { "type": "string" },
                        "depth": { "type": "integer", "minimum": 0, "maximum": 5, "default": 2 },
                        "limit": { "type": "integer", "minimum": 1, "maximum": 100, "default": 12 }
                    },
                    "required": ["query"]
                }
            },
            {
                "name": "ckb_symbol_memory",
                "description": "Retrieve architecture memory centered on one symbol ID/name/path. Useful before editing a function, method, class, interface, service, or file.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "symbol": { "type": "string" },
                        "depth": { "type": "integer", "minimum": 0, "maximum": 5, "default": 2 },
                        "limit": { "type": "integer", "minimum": 1, "maximum": 100, "default": 8 }
                    },
                    "required": ["symbol"]
                }
            },
            {
                "name": "ckb_code_dna",
                "description": "Return explainable Code DNA health/risk metrics derived from graph topology, cycles and observed runtime telemetry. Scores are heuristics, not fabricated learned probabilities.",
                "inputSchema": { "type": "object", "properties": {} }
            },
            {
                "name": "ckb_causal_path",
                "description": "Explain the shortest proven directed architecture path from one exact CKB node ID to another, including relationship evidence and explicit runtime-observation flags.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "source": { "type": "string", "description": "Exact CKB node ID, for example src/auth.ts::login" },
                        "target": { "type": "string", "description": "Exact CKB node ID" },
                        "max_depth": { "type": "integer", "minimum": 1, "maximum": 32, "default": 12 }
                    },
                    "required": ["source", "target"]
                }
            },
            {
                "name": "ckb_failure_cone",
                "description": "Return the real transitive upstream dependent cone for a node. This is change/failure propagation evidence from the current graph, not a claim that a runtime failure has occurred.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "root": { "type": "string", "description": "Exact CKB node ID" },
                        "max_depth": { "type": "integer", "minimum": 1, "maximum": 32, "default": 12 }
                    },
                    "required": ["root"]
                }
            }
        ]
    })
}

fn text_result(value: Value) -> Value {
    json!({
        "content": [{ "type": "text", "text": serde_json::to_string_pretty(&value).unwrap_or_else(|_| "{}".into()) }],
        "isError": false
    })
}

fn error_result(message: impl Into<String>) -> Value {
    json!({
        "content": [{ "type": "text", "text": message.into() }],
        "isError": true
    })
}

async fn call_tool(session: &Arc<RwLock<MemorySession>>, name: &str, args: &Value) -> Value {
    match name {
        "ckb_memory_scan" => {
            let Some(path) = args.get("path").and_then(Value::as_str) else { return error_result("path is required"); };
            match build_graph(path).await {
                Ok((graph, files)) => {
                    let persisted = match persist_memory(path, &graph) {
                        Ok(value) => value,
                        Err(e) => return error_result(format!("Architecture scan succeeded but durable memory persistence failed: {e}")),
                    };
                    let summary = json!({
                        "status": "remembered",
                        "path": persisted.root,
                        "filesProcessed": files,
                        "nodes": graph.node_count(),
                        "edges": graph.edge_count(),
                        "savedAt": persisted.saved_at,
                        "gitHead": persisted.git_head,
                        "memoryFile": memory_path(path).to_string_lossy(),
                        "evidencePolicy": "static-runtime-predicted-separated",
                        "memoryLifetime": "durable across MCP process restarts"
                    });
                    let mut state = session.write().await;
                    state.graph = Some(graph);
                    state.root = Some(persisted.root);
                    state.git_head = persisted.git_head;
                    state.saved_at = Some(persisted.saved_at);
                    text_result(summary)
                }
                Err(e) => error_result(format!("CKB memory scan failed: {e}")),
            }
        }
        "ckb_memory_resume" => {
            let Some(path) = args.get("path").and_then(Value::as_str) else { return error_result("path is required"); };
            match load_memory(path) {
                Ok(persisted) => {
                    let (freshness_state, current_head) = freshness(&persisted.root, persisted.git_head.as_deref());
                    let nodes = persisted.graph.node_count();
                    let edges = persisted.graph.edge_count();
                    let response = json!({
                        "status": "resumed",
                        "path": persisted.root,
                        "nodes": nodes,
                        "edges": edges,
                        "savedAt": persisted.saved_at,
                        "savedGitHead": persisted.git_head,
                        "currentGitHead": current_head,
                        "freshness": freshness_state,
                        "mustRescanBeforeHighConfidenceEdits": freshness_state == "stale",
                        "memoryFile": memory_path(path).to_string_lossy(),
                        "synthetic": false
                    });
                    let mut state = session.write().await;
                    state.graph = Some(persisted.graph);
                    state.root = Some(persisted.root);
                    state.git_head = persisted.git_head;
                    state.saved_at = Some(persisted.saved_at);
                    text_result(response)
                }
                Err(e) => error_result(format!("CKB durable memory resume failed: {e}")),
            }
        }
        "ckb_memory_status" => {
            let state = session.read().await;
            let Some(graph) = state.graph.as_ref() else {
                return text_result(json!({
                    "loaded": false,
                    "message": "No architecture memory is loaded in this MCP process. Use ckb_memory_resume or ckb_memory_scan.",
                    "memoryDirectory": memory_base_dir().to_string_lossy(),
                    "synthetic": false
                }));
            };
            let root = state.root.as_deref().unwrap_or("");
            let (freshness_state, current_head) = freshness(root, state.git_head.as_deref());
            text_result(json!({
                "loaded": true,
                "path": root,
                "nodes": graph.node_count(),
                "edges": graph.edge_count(),
                "savedAt": state.saved_at,
                "savedGitHead": state.git_head,
                "currentGitHead": current_head,
                "freshness": freshness_state,
                "memoryFile": memory_path(root).to_string_lossy(),
                "synthetic": false
            }))
        }
        "ckb_memory_query" | "ckb_symbol_memory" => {
            let state = session.read().await;
            let Some(graph) = state.graph.as_ref() else { return error_result("No architecture memory loaded. Call ckb_memory_resume or ckb_memory_scan first."); };
            let query_key = if name == "ckb_symbol_memory" { "symbol" } else { "query" };
            let query = args.get(query_key).and_then(Value::as_str).unwrap_or("");
            if query.is_empty() { return error_result(format!("{query_key} is required")); }
            let depth = args.get("depth").and_then(Value::as_u64).unwrap_or(2) as usize;
            let limit = args.get("limit").and_then(Value::as_u64).unwrap_or(if name == "ckb_symbol_memory" { 8 } else { 12 }) as usize;
            match ArchitectureMemoryEngine::query(graph, query, depth, limit) {
                Ok(slice) => text_result(serde_json::to_value(slice).unwrap_or(Value::Null)),
                Err(e) => error_result(format!("Architecture memory retrieval failed: {e}")),
            }
        }
        "ckb_code_dna" => {
            let state = session.read().await;
            let Some(graph) = state.graph.as_ref() else { return error_result("No architecture memory loaded. Call ckb_memory_resume or ckb_memory_scan first."); };
            match ArchitectureMemoryEngine::code_dna(graph) {
                Ok(report) => text_result(serde_json::to_value(report).unwrap_or(Value::Null)),
                Err(e) => error_result(format!("Code DNA analysis failed: {e}")),
            }
        }
        "ckb_causal_path" => {
            let state = session.read().await;
            let Some(graph) = state.graph.as_ref() else { return error_result("No architecture memory loaded. Call ckb_memory_resume or ckb_memory_scan first."); };
            let Some(source) = args.get("source").and_then(Value::as_str) else { return error_result("source is required"); };
            let Some(target) = args.get("target").and_then(Value::as_str) else { return error_result("target is required"); };
            let max_depth = args.get("max_depth").and_then(Value::as_u64).unwrap_or(12) as usize;
            match CausalArchitectureEngine::shortest_path(graph, &NodeId(source.to_string()), &NodeId(target.to_string()), max_depth) {
                Ok(report) => text_result(serde_json::to_value(report).unwrap_or(Value::Null)),
                Err(e) => error_result(format!("Causal path analysis failed: {e}")),
            }
        }
        "ckb_failure_cone" => {
            let state = session.read().await;
            let Some(graph) = state.graph.as_ref() else { return error_result("No architecture memory loaded. Call ckb_memory_resume or ckb_memory_scan first."); };
            let Some(root) = args.get("root").and_then(Value::as_str) else { return error_result("root is required"); };
            let max_depth = args.get("max_depth").and_then(Value::as_u64).unwrap_or(12) as usize;
            match CausalArchitectureEngine::failure_cone(graph, &NodeId(root.to_string()), max_depth) {
                Ok(report) => text_result(serde_json::to_value(report).unwrap_or(Value::Null)),
                Err(e) => error_result(format!("Failure-cone analysis failed: {e}")),
            }
        }
        _ => error_result(format!("Unknown tool: {name}")),
    }
}

async fn handle(session: &Arc<RwLock<MemorySession>>, request: Value) -> Value {
    let id = request.get("id").cloned().unwrap_or(Value::Null);
    let method = request.get("method").and_then(Value::as_str).unwrap_or("");
    let result = match method {
        "initialize" => json!({
            "protocolVersion": "2024-11-05",
            "capabilities": { "tools": {} },
            "serverInfo": { "name": "ckb-architecture-memory", "version": "2.2.0" },
            "instructions": "Use ckb_memory_resume first for a repository that CKB has seen before. Rescan when freshness is stale. Then query bounded architecture memory, Code DNA, causal paths, or failure cones before edits so models reason from durable CKB evidence instead of rediscovering or guessing the codebase."
        }),
        "notifications/initialized" => return Value::Null,
        "tools/list" => tool_list(),
        "tools/call" => {
            let params = request.get("params").cloned().unwrap_or_else(|| json!({}));
            let name = params.get("name").and_then(Value::as_str).unwrap_or("");
            let args = params.get("arguments").cloned().unwrap_or_else(|| json!({}));
            call_tool(session, name, &args).await
        }
        "ping" => json!({}),
        _ => return json!({ "jsonrpc": "2.0", "id": id, "error": { "code": -32601, "message": format!("Method not found: {method}") } }),
    };
    json!({ "jsonrpc": "2.0", "id": id, "result": result })
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
            Ok(request) => handle(&session, request).await,
            Err(e) => json!({ "jsonrpc": "2.0", "id": Value::Null, "error": { "code": -32700, "message": format!("Parse error: {e}") } }),
        };
        if response.is_null() { continue; }
        stdout.write_all(serde_json::to_string(&response)?.as_bytes()).await?;
        stdout.write_all(b"\n").await?;
        stdout.flush().await?;
    }
    Ok(())
}
