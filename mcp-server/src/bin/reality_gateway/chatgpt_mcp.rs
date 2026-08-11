use super::{authorized, json_response, GatewayState};
use axum::{
    body::Body,
    extract::State,
    http::{header, HeaderMap, Response, StatusCode},
    Json,
};
use reqwest::Method;
use serde_json::{json, Value};

const DEFAULT_PROTOCOL_VERSION: &str = "2025-06-18";
const SUPPORTED_PROTOCOL_VERSIONS: &[&str] = &[
    "2025-11-25",
    "2025-06-18",
    "2025-03-26",
    "2024-11-05",
];

pub(super) async fn get_mcp(
    State(state): State<GatewayState>,
    headers: HeaderMap,
) -> Response<Body> {
    if let Err((status, message)) = authorized(&state, &headers) {
        return json_response(status, json!({ "error": message }));
    }

    Response::builder()
        .status(StatusCode::METHOD_NOT_ALLOWED)
        .header(header::ALLOW, "POST")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(
            json!({
                "message": "CKB MCP uses stateless Streamable HTTP. Send MCP JSON-RPC messages with POST /mcp."
            })
            .to_string(),
        ))
        .unwrap_or_else(|_| Response::new(Body::empty()))
}

pub(super) async fn post_mcp(
    State(state): State<GatewayState>,
    headers: HeaderMap,
    Json(request): Json<Value>,
) -> Response<Body> {
    if let Err((status, message)) = authorized(&state, &headers) {
        return json_response(status, json!({ "error": message }));
    }

    let method = request
        .get("method")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let id = request.get("id").cloned();

    // MCP notifications do not receive JSON-RPC responses.
    if id.is_none() {
        return match method {
            "notifications/initialized" | "notifications/cancelled" => Response::builder()
                .status(StatusCode::ACCEPTED)
                .body(Body::empty())
                .unwrap_or_else(|_| Response::new(Body::empty())),
            _ => Response::builder()
                .status(StatusCode::ACCEPTED)
                .body(Body::empty())
                .unwrap_or_else(|_| Response::new(Body::empty())),
        };
    }

    let id = id.unwrap_or(Value::Null);
    match method {
        "initialize" => {
            let requested = request
                .pointer("/params/protocolVersion")
                .and_then(Value::as_str)
                .unwrap_or(DEFAULT_PROTOCOL_VERSION);
            let negotiated = if SUPPORTED_PROTOCOL_VERSIONS.contains(&requested) {
                requested
            } else {
                DEFAULT_PROTOCOL_VERSION
            };
            rpc_result(
                id,
                json!({
                    "protocolVersion": negotiated,
                    "capabilities": {
                        "tools": { "listChanged": false }
                    },
                    "serverInfo": {
                        "name": "ckb-chatgpt-mcp",
                        "title": "CKB Architecture Intelligence",
                        "version": env!("CARGO_PKG_VERSION")
                    },
                    "instructions": "Use CKB to scan a public GitHub repository into a named project, then inspect architecture, blast radius, runtime evidence, history, test gaps, causal paths, failure cones, snapshots, and architecture memory. CKB analysis tools do not modify the target repository."
                }),
            )
        }
        "ping" => rpc_result(id, json!({})),
        "tools/list" => rpc_result(id, json!({ "tools": tools() })),
        "tools/call" => handle_tool_call(&state, id, request.get("params")).await,
        _ => rpc_error(id, -32601, format!("Method not found: {method}")),
    }
}

async fn handle_tool_call(
    state: &GatewayState,
    id: Value,
    params: Option<&Value>,
) -> Response<Body> {
    let name = params
        .and_then(|value| value.get("name"))
        .and_then(Value::as_str)
        .unwrap_or_default();
    let args = params
        .and_then(|value| value.get("arguments"))
        .cloned()
        .unwrap_or_else(|| json!({}));

    if !tools().iter().any(|tool| {
        tool.get("name")
            .and_then(Value::as_str)
            .map(|candidate| candidate == name)
            .unwrap_or(false)
    }) {
        return rpc_error(id, -32602, format!("Unknown CKB tool: {name}"));
    }

    match execute_tool(state, name, &args).await {
        Ok(value) => {
            let structured = if value.is_object() {
                value
            } else {
                json!({ "value": value })
            };
            let text = serde_json::to_string_pretty(&structured)
                .unwrap_or_else(|_| structured.to_string());
            rpc_result(
                id,
                json!({
                    "content": [{ "type": "text", "text": text }],
                    "structuredContent": structured,
                    "isError": false
                }),
            )
        }
        Err(message) => rpc_result(
            id,
            json!({
                "content": [{ "type": "text", "text": message }],
                "isError": true
            }),
        ),
    }
}

async fn execute_tool(
    state: &GatewayState,
    name: &str,
    args: &Value,
) -> Result<Value, String> {
    match name {
        "ckb_scan_repository" => {
            let github_url = required_str(args, "github_url")?;
            let project_id = required_str(args, "project_id")?;
            upstream(
                state,
                Method::POST,
                "/api/v1/intelligence/scan/github".into(),
                Some(json!({
                    "github_url": github_url,
                    "project_id": project_id,
                    "repo_name": project_id
                })),
            )
            .await
        }
        "ckb_get_architecture_graph" => {
            let project_id = required_str(args, "project_id")?;
            upstream(
                state,
                Method::GET,
                format!(
                    "/api/v1/intelligence/graph?project_id={}",
                    encode_component(project_id)
                ),
                None,
            )
            .await
        }
        "ckb_analyze_impact" => {
            let project_id = required_str(args, "project_id")?;
            let file = required_str(args, "file")?;
            let line = args.get("line").and_then(Value::as_u64).unwrap_or(1);
            let change_type = args
                .get("change_type")
                .and_then(Value::as_str)
                .unwrap_or("modify");
            upstream(
                state,
                Method::POST,
                "/api/v1/intelligence/impact".into(),
                Some(json!({
                    "project_id": project_id,
                    "repo_name": project_id,
                    "file": file,
                    "line": line,
                    "change_type": change_type
                })),
            )
            .await
        }
        "ckb_get_runtime_intelligence" => {
            let project_id = required_str(args, "project_id")?;
            upstream(
                state,
                Method::GET,
                format!(
                    "/api/v1/intelligence/runtime?project_id={}",
                    encode_component(project_id)
                ),
                None,
            )
            .await
        }
        "ckb_get_drift_history" => {
            let project_id = required_str(args, "project_id")?;
            let max_commits = args
                .get("max_commits")
                .and_then(Value::as_u64)
                .unwrap_or(50)
                .min(500);
            upstream(
                state,
                Method::GET,
                format!(
                    "/api/v1/intelligence/history?project_id={}&max_commits={}",
                    encode_component(project_id),
                    max_commits
                ),
                None,
            )
            .await
        }
        "ckb_get_test_gaps" => {
            let project_id = required_str(args, "project_id")?;
            upstream(
                state,
                Method::GET,
                format!(
                    "/api/v1/test-gaps?project_id={}",
                    encode_component(project_id)
                ),
                None,
            )
            .await
        }
        "ckb_find_causal_path" => {
            let project_id = required_str(args, "project_id")?;
            let source = required_str(args, "source")?;
            let target = required_str(args, "target")?;
            let max_depth = args.get("max_depth").and_then(Value::as_u64).unwrap_or(12);
            upstream(
                state,
                Method::POST,
                "/api/v1/intelligence/causal-path".into(),
                Some(json!({
                    "project_id": project_id,
                    "repo_name": project_id,
                    "source": source,
                    "target": target,
                    "max_depth": max_depth
                })),
            )
            .await
        }
        "ckb_get_failure_cone" => {
            let project_id = required_str(args, "project_id")?;
            let root = required_str(args, "root")?;
            let max_depth = args.get("max_depth").and_then(Value::as_u64).unwrap_or(12);
            upstream(
                state,
                Method::POST,
                "/api/v1/intelligence/failure-cone".into(),
                Some(json!({
                    "project_id": project_id,
                    "repo_name": project_id,
                    "root": root,
                    "max_depth": max_depth
                })),
            )
            .await
        }
        "ckb_query_architecture_memory" => {
            let project_id = required_str(args, "project_id")?;
            let query = required_str(args, "query")?;
            let depth = args.get("depth").and_then(Value::as_u64).unwrap_or(2);
            let limit = args.get("limit").and_then(Value::as_u64).unwrap_or(12);
            upstream(
                state,
                Method::POST,
                "/api/v1/intelligence/memory/query".into(),
                Some(json!({
                    "project_id": project_id,
                    "repo_name": project_id,
                    "query": query,
                    "depth": depth,
                    "limit": limit
                })),
            )
            .await
        }
        "ckb_get_code_dna" => {
            let project_id = required_str(args, "project_id")?;
            upstream(
                state,
                Method::GET,
                format!(
                    "/api/v1/intelligence/code-dna?project_id={}",
                    encode_component(project_id)
                ),
                None,
            )
            .await
        }
        "ckb_list_snapshots" => {
            let project_id = required_str(args, "project_id")?;
            upstream(
                state,
                Method::GET,
                format!(
                    "/api/v1/intelligence/snapshots?project_id={}",
                    encode_component(project_id)
                ),
                None,
            )
            .await
        }
        "ckb_diff_snapshots" => {
            let project_id = required_str(args, "project_id")?;
            let from_snapshot = required_str(args, "from_snapshot")?;
            let to_snapshot = required_str(args, "to_snapshot")?;
            upstream(
                state,
                Method::POST,
                "/api/v1/intelligence/diff".into(),
                Some(json!({
                    "project_id": project_id,
                    "repo_name": project_id,
                    "from_snapshot": from_snapshot,
                    "to_snapshot": to_snapshot
                })),
            )
            .await
        }
        "ckb_generate_ai_rules" => {
            let project_id = required_str(args, "project_id")?;
            upstream(
                state,
                Method::GET,
                format!(
                    "/api/v1/rules?project_id={}",
                    encode_component(project_id)
                ),
                None,
            )
            .await
        }
        _ => Err(format!("Unsupported CKB tool: {name}")),
    }
}

async fn upstream(
    state: &GatewayState,
    method: Method,
    path: String,
    body: Option<Value>,
) -> Result<Value, String> {
    let mut request = state
        .client
        .request(method, format!("{}{}", state.child_base_url.as_str(), path))
        .header(reqwest::header::ACCEPT, "application/json, text/plain");

    if let Some(api_key) = &state.api_key {
        request = request.header("x-api-key", api_key.as_str());
    }
    if let Some(body) = body {
        request = request.json(&body);
    }

    let response = request
        .send()
        .await
        .map_err(|error| format!("CKB Reality request failed: {error}"))?;
    let status = response.status();
    let text = response
        .text()
        .await
        .map_err(|error| format!("CKB Reality response failed: {error}"))?;

    if !status.is_success() {
        return Err(format!("CKB Reality returned {status}: {text}"));
    }

    serde_json::from_str::<Value>(&text).or_else(|_| Ok(json!({ "text": text })))
}

fn required_str<'a>(args: &'a Value, key: &str) -> Result<&'a str, String> {
    args.get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| format!("Missing required argument: {key}"))
}

fn encode_component(value: &str) -> String {
    let mut output = String::with_capacity(value.len());
    for byte in value.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                output.push(byte as char)
            }
            _ => output.push_str(&format!("%{byte:02X}")),
        }
    }
    output
}

fn rpc_result(id: Value, result: Value) -> Response<Body> {
    json_response(
        StatusCode::OK,
        json!({ "jsonrpc": "2.0", "id": id, "result": result }),
    )
}

fn rpc_error(id: Value, code: i64, message: String) -> Response<Body> {
    json_response(
        StatusCode::OK,
        json!({
            "jsonrpc": "2.0",
            "id": id,
            "error": { "code": code, "message": message }
        }),
    )
}

fn read_only_annotations(open_world: bool) -> Value {
    json!({
        "title": "CKB read-only analysis",
        "readOnlyHint": true,
        "destructiveHint": false,
        "idempotentHint": true,
        "openWorldHint": open_world
    })
}

fn tools() -> Vec<Value> {
    vec![
        json!({
            "name": "ckb_scan_repository",
            "title": "Scan repository with CKB",
            "description": "Read and analyze a public GitHub repository with CKB Reality, build its persistent architecture graph, and create an architecture snapshot. This does not modify the target repository. Use a stable project_id so later CKB calls address the same analyzed project.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "github_url": { "type": "string", "description": "Public GitHub repository URL, for example https://github.com/owner/repo" },
                    "project_id": { "type": "string", "description": "Stable CKB project key for this analysis, for example owner-repo" }
                },
                "required": ["github_url", "project_id"],
                "additionalProperties": false
            },
            "annotations": read_only_annotations(true)
        }),
        json!({
            "name": "ckb_get_architecture_graph",
            "title": "Get architecture graph",
            "description": "Return CKB's persisted static architecture graph for a previously scanned project, including nodes, edges, evidence, and architecture metadata.",
            "inputSchema": project_schema(),
            "annotations": read_only_annotations(false)
        }),
        json!({
            "name": "ckb_analyze_impact",
            "title": "Analyze blast radius",
            "description": "Calculate the architectural blast radius and risk of changing a file at a specific line in a previously scanned project.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "project_id": { "type": "string" },
                    "file": { "type": "string", "description": "Repository-relative file path or CKB node file path" },
                    "line": { "type": "integer", "minimum": 1, "default": 1 },
                    "change_type": { "type": "string", "enum": ["add", "modify", "delete", "rename"], "default": "modify" }
                },
                "required": ["project_id", "file"],
                "additionalProperties": false
            },
            "annotations": read_only_annotations(false)
        }),
        json!({
            "name": "ckb_get_runtime_intelligence",
            "title": "Get runtime intelligence",
            "description": "Return observed OpenTelemetry-backed runtime nodes and edges for a CKB project, including invocation counts, latency, error rates, and runtime evidence.",
            "inputSchema": project_schema(),
            "annotations": read_only_annotations(false)
        }),
        json!({
            "name": "ckb_get_drift_history",
            "title": "Get architecture history",
            "description": "Inspect Git-backed architectural history and drift evidence for a CKB project.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "project_id": { "type": "string" },
                    "max_commits": { "type": "integer", "minimum": 1, "maximum": 500, "default": 50 }
                },
                "required": ["project_id"],
                "additionalProperties": false
            },
            "annotations": read_only_annotations(true)
        }),
        json!({
            "name": "ckb_get_test_gaps",
            "title": "Find test coverage gaps",
            "description": "Use CKB's graph-aware test analysis to identify important architecture paths and nodes that lack test coverage.",
            "inputSchema": project_schema(),
            "annotations": read_only_annotations(false)
        }),
        json!({
            "name": "ckb_find_causal_path",
            "title": "Find causal architecture path",
            "description": "Find the shortest architecture path between two CKB node IDs, with evidence for how one component can influence another.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "project_id": { "type": "string" },
                    "source": { "type": "string", "description": "CKB source node ID" },
                    "target": { "type": "string", "description": "CKB target node ID" },
                    "max_depth": { "type": "integer", "minimum": 1, "maximum": 64, "default": 12 }
                },
                "required": ["project_id", "source", "target"],
                "additionalProperties": false
            },
            "annotations": read_only_annotations(false)
        }),
        json!({
            "name": "ckb_get_failure_cone",
            "title": "Trace failure cone",
            "description": "Trace the transitive downstream failure cone from a CKB node ID to show what can be affected if that component fails or changes.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "project_id": { "type": "string" },
                    "root": { "type": "string", "description": "CKB root node ID" },
                    "max_depth": { "type": "integer", "minimum": 1, "maximum": 64, "default": 12 }
                },
                "required": ["project_id", "root"],
                "additionalProperties": false
            },
            "annotations": read_only_annotations(false)
        }),
        json!({
            "name": "ckb_query_architecture_memory",
            "title": "Query architecture memory",
            "description": "Ask CKB Architecture Memory for a graph-grounded context slice related to an architectural concept, file, component, or symbol.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "project_id": { "type": "string" },
                    "query": { "type": "string" },
                    "depth": { "type": "integer", "minimum": 1, "maximum": 8, "default": 2 },
                    "limit": { "type": "integer", "minimum": 1, "maximum": 100, "default": 12 }
                },
                "required": ["project_id", "query"],
                "additionalProperties": false
            },
            "annotations": read_only_annotations(false)
        }),
        json!({
            "name": "ckb_get_code_dna",
            "title": "Get code DNA",
            "description": "Return CKB's architecture-memory Code DNA summary for a scanned project.",
            "inputSchema": project_schema(),
            "annotations": read_only_annotations(false)
        }),
        json!({
            "name": "ckb_list_snapshots",
            "title": "List architecture snapshots",
            "description": "List persistent architecture snapshots captured for a CKB project.",
            "inputSchema": project_schema(),
            "annotations": read_only_annotations(false)
        }),
        json!({
            "name": "ckb_diff_snapshots",
            "title": "Diff architecture snapshots",
            "description": "Compare two persistent CKB architecture snapshots and return node, edge, and violation deltas.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "project_id": { "type": "string" },
                    "from_snapshot": { "type": "string" },
                    "to_snapshot": { "type": "string" }
                },
                "required": ["project_id", "from_snapshot", "to_snapshot"],
                "additionalProperties": false
            },
            "annotations": read_only_annotations(false)
        }),
        json!({
            "name": "ckb_generate_ai_rules",
            "title": "Generate architecture guardrails",
            "description": "Generate AI coding guidelines and architecture guardrails from the project's current CKB graph. The rules are returned as text and are not written into the repository.",
            "inputSchema": project_schema(),
            "annotations": read_only_annotations(false)
        }),
    ]
}

fn project_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "project_id": { "type": "string", "description": "CKB project key used when the repository was scanned" }
        },
        "required": ["project_id"],
        "additionalProperties": false
    })
}
