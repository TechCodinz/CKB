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
const ARCHITECTURE_SCOPE: &str = "architecture:read";
const REPOSITORY_SCAN_SCOPE: &str = "repository:scan";
const OFFLINE_SCOPE: &str = "offline_access";

#[derive(Clone, Debug)]
struct AuthContext {
    // OAuth callers are isolated into a per-user Reality project namespace.
    // Trusted internal/operator credentials preserve the original project key.
    user_id: Option<String>,
}

pub(super) async fn get_mcp() -> Response<Body> {
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

pub(super) async fn oauth_protected_resource() -> Response<Body> {
    json_response(
        StatusCode::OK,
        json!({
            "resource": mcp_resource(),
            "authorization_servers": [oauth_issuer()],
            "scopes_supported": [ARCHITECTURE_SCOPE, REPOSITORY_SCAN_SCOPE, OFFLINE_SCOPE],
            "resource_documentation": "https://ckb-nu.vercel.app"
        }),
    )
}

pub(super) async fn post_mcp(
    State(state): State<GatewayState>,
    headers: HeaderMap,
    Json(request): Json<Value>,
) -> Response<Body> {
    let method = request
        .get("method")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let id = request.get("id").cloned();

    // MCP notifications do not receive JSON-RPC responses. Initialization and
    // discovery stay public so ChatGPT can see OAuth security schemes before a
    // user links their CKB account; every tool execution is authorized below.
    if id.is_none() {
        return Response::builder()
            .status(StatusCode::ACCEPTED)
            .body(Body::empty())
            .unwrap_or_else(|_| Response::new(Body::empty()));
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
                    "instructions": "Use CKB to scan a GitHub repository into the caller's isolated CKB project namespace, then inspect architecture, blast radius, runtime evidence, history, test gaps, causal paths, failure cones, snapshots, and architecture memory. CKB analysis tools do not modify the target repository."
                }),
            )
        }
        "ping" => rpc_result(id, json!({})),
        "tools/list" => rpc_result(id, json!({ "tools": tools() })),
        "tools/call" => handle_tool_call(&state, &headers, id, request.get("params")).await,
        _ => rpc_error(id, -32601, format!("Method not found: {method}")),
    }
}

async fn handle_tool_call(
    state: &GatewayState,
    headers: &HeaderMap,
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

    let required = match required_scope(name) {
        Some(scope) => scope,
        None => return rpc_error(id, -32602, format!("Unknown CKB tool: {name}")),
    };

    let auth = match authorize_tool(state, headers, required).await {
        Ok(auth) => auth,
        Err(reason) => return oauth_required_result(id, required, &reason),
    };

    match execute_tool(state, &auth, name, &args).await {
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

fn required_scope(name: &str) -> Option<&'static str> {
    match name {
        "ckb_scan_repository" => Some(REPOSITORY_SCAN_SCOPE),
        "ckb_get_architecture_graph"
        | "ckb_analyze_impact"
        | "ckb_get_runtime_intelligence"
        | "ckb_get_drift_history"
        | "ckb_get_test_gaps"
        | "ckb_find_causal_path"
        | "ckb_get_failure_cone"
        | "ckb_query_architecture_memory"
        | "ckb_get_code_dna"
        | "ckb_list_snapshots"
        | "ckb_diff_snapshots"
        | "ckb_generate_ai_rules" => Some(ARCHITECTURE_SCOPE),
        _ => None,
    }
}

async fn authorize_tool(
    state: &GatewayState,
    headers: &HeaderMap,
    required_scope: &str,
) -> Result<AuthContext, String> {
    // Preserve trusted infrastructure/operator access for deployment probes and
    // API-based development. ChatGPT itself cannot present custom API keys; its
    // end-user path proceeds through OAuth introspection below.
    if authorized(state, headers).is_ok() {
        return Ok(AuthContext { user_id: None });
    }

    let bearer = headers
        .get(header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "))
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| "No linked CKB OAuth access token was provided.".to_string())?;

    let internal_secret = state
        .internal_secret
        .as_ref()
        .ok_or_else(|| "CKB OAuth introspection is not configured on this deployment.".to_string())?;
    let introspection_url = format!("{}/oauth/introspect", oauth_issuer());
    let response = state
        .client
        .post(introspection_url)
        .header("x-ckb-internal-secret", internal_secret.as_str())
        .json(&json!({ "token": bearer }))
        .send()
        .await
        .map_err(|error| format!("CKB OAuth validation is temporarily unavailable: {error}"))?;

    if !response.status().is_success() {
        return Err(format!(
            "CKB OAuth validation returned {}.",
            response.status()
        ));
    }
    let payload: Value = response
        .json()
        .await
        .map_err(|error| format!("CKB OAuth validation returned invalid JSON: {error}"))?;
    if payload.get("active").and_then(Value::as_bool) != Some(true) {
        return Err("The CKB OAuth token is invalid, expired, or revoked.".into());
    }

    let scopes = payload
        .get("scope")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .split_whitespace()
        .collect::<Vec<_>>();
    if !scopes.contains(&required_scope) {
        return Err(format!(
            "The linked CKB account has not granted the required {required_scope} scope."
        ));
    }
    let user_id = payload
        .get("sub")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| "CKB OAuth validation did not return a user identity.".to_string())?;

    Ok(AuthContext {
        user_id: Some(user_id.to_string()),
    })
}

fn oauth_required_result(id: Value, required_scope: &str, reason: &str) -> Response<Body> {
    let challenge = format!(
        "Bearer resource_metadata=\"{}/.well-known/oauth-protected-resource\", scope=\"{} {}\", error=\"insufficient_scope\", error_description=\"Link your CKB account to continue\"",
        mcp_resource(),
        required_scope,
        OFFLINE_SCOPE
    );
    rpc_result(
        id,
        json!({
            "content": [{
                "type": "text",
                "text": format!("CKB authentication required: {reason}")
            }],
            "_meta": {
                "mcp/www_authenticate": [challenge]
            },
            "isError": true
        }),
    )
}

fn mcp_resource() -> String {
    std::env::var("CKB_MCP_RESOURCE")
        .unwrap_or_else(|_| "https://ckb-mcp-server.onrender.com".into())
        .trim_end_matches('/')
        .to_string()
}

fn oauth_issuer() -> String {
    std::env::var("CKB_BACKEND_URL")
        .unwrap_or_else(|_| "https://ckb-backend-api.onrender.com".into())
        .trim_end_matches('/')
        .to_string()
}

fn scoped_project_id(auth: &AuthContext, logical_project_id: &str) -> String {
    match &auth.user_id {
        Some(user_id) => format!("chatgpt:{user_id}:{logical_project_id}"),
        None => logical_project_id.to_string(),
    }
}

async fn execute_tool(
    state: &GatewayState,
    auth: &AuthContext,
    name: &str,
    args: &Value,
) -> Result<Value, String> {
    match name {
        "ckb_scan_repository" => {
            let github_url = required_str(args, "github_url")?;
            let logical_project_id = required_str(args, "project_id")?;
            let project_id = scoped_project_id(auth, logical_project_id);
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
            let project_id = scoped_project_id(auth, required_str(args, "project_id")?);
            upstream(
                state,
                Method::GET,
                format!(
                    "/api/v1/intelligence/graph?project_id={}",
                    encode_component(&project_id)
                ),
                None,
            )
            .await
        }
        "ckb_analyze_impact" => {
            let project_id = scoped_project_id(auth, required_str(args, "project_id")?);
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
            let project_id = scoped_project_id(auth, required_str(args, "project_id")?);
            upstream(
                state,
                Method::GET,
                format!(
                    "/api/v1/intelligence/runtime?project_id={}",
                    encode_component(&project_id)
                ),
                None,
            )
            .await
        }
        "ckb_get_drift_history" => {
            let project_id = scoped_project_id(auth, required_str(args, "project_id")?);
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
                    encode_component(&project_id),
                    max_commits
                ),
                None,
            )
            .await
        }
        "ckb_get_test_gaps" => {
            let project_id = scoped_project_id(auth, required_str(args, "project_id")?);
            upstream(
                state,
                Method::GET,
                format!(
                    "/api/v1/test-gaps?project_id={}",
                    encode_component(&project_id)
                ),
                None,
            )
            .await
        }
        "ckb_find_causal_path" => {
            let project_id = scoped_project_id(auth, required_str(args, "project_id")?);
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
            let project_id = scoped_project_id(auth, required_str(args, "project_id")?);
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
            let project_id = scoped_project_id(auth, required_str(args, "project_id")?);
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
            let project_id = scoped_project_id(auth, required_str(args, "project_id")?);
            upstream(
                state,
                Method::GET,
                format!(
                    "/api/v1/intelligence/code-dna?project_id={}",
                    encode_component(&project_id)
                ),
                None,
            )
            .await
        }
        "ckb_list_snapshots" => {
            let project_id = scoped_project_id(auth, required_str(args, "project_id")?);
            upstream(
                state,
                Method::GET,
                format!(
                    "/api/v1/intelligence/snapshots?project_id={}",
                    encode_component(&project_id)
                ),
                None,
            )
            .await
        }
        "ckb_diff_snapshots" => {
            let project_id = scoped_project_id(auth, required_str(args, "project_id")?);
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
            let project_id = scoped_project_id(auth, required_str(args, "project_id")?);
            upstream(
                state,
                Method::GET,
                format!(
                    "/api/v1/rules?project_id={}",
                    encode_component(&project_id)
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

fn security(scope: &'static str) -> Value {
    json!([{ "type": "oauth2", "scopes": [scope, OFFLINE_SCOPE] }])
}

fn project_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "project_id": { "type": "string", "minLength": 1, "maxLength": 160, "description": "Logical CKB project key. OAuth users are automatically isolated into a private per-user CKB namespace." }
        },
        "required": ["project_id"],
        "additionalProperties": false
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
                    "github_url": { "type": "string", "minLength": 12, "maxLength": 2048, "description": "Public GitHub repository URL, for example https://github.com/owner/repo" },
                    "project_id": { "type": "string", "minLength": 1, "maxLength": 160, "description": "Stable logical CKB project key for this analysis, for example owner-repo" }
                },
                "required": ["github_url", "project_id"],
                "additionalProperties": false
            },
            "securitySchemes": security(REPOSITORY_SCAN_SCOPE),
            "annotations": read_only_annotations(true)
        }),
        json!({
            "name": "ckb_get_architecture_graph",
            "title": "Get architecture graph",
            "description": "Return CKB's persisted static architecture graph for a previously scanned project, including nodes, edges, evidence, and architecture metadata.",
            "inputSchema": project_schema(),
            "securitySchemes": security(ARCHITECTURE_SCOPE),
            "annotations": read_only_annotations(false)
        }),
        json!({
            "name": "ckb_analyze_impact",
            "title": "Analyze blast radius",
            "description": "Calculate the architectural blast radius and risk of changing a file at a specific line in a previously scanned project.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "project_id": { "type": "string", "minLength": 1, "maxLength": 160 },
                    "file": { "type": "string", "minLength": 1, "maxLength": 2048, "description": "Repository-relative file path or CKB node file path" },
                    "line": { "type": "integer", "minimum": 1, "default": 1 },
                    "change_type": { "type": "string", "enum": ["add", "modify", "delete", "rename"], "default": "modify" }
                },
                "required": ["project_id", "file"],
                "additionalProperties": false
            },
            "securitySchemes": security(ARCHITECTURE_SCOPE),
            "annotations": read_only_annotations(false)
        }),
        json!({
            "name": "ckb_get_runtime_intelligence",
            "title": "Get runtime intelligence",
            "description": "Return observed OpenTelemetry-backed runtime nodes and edges for a CKB project, including invocation counts, latency, error rates, and runtime evidence.",
            "inputSchema": project_schema(),
            "securitySchemes": security(ARCHITECTURE_SCOPE),
            "annotations": read_only_annotations(false)
        }),
        json!({
            "name": "ckb_get_drift_history",
            "title": "Get architecture history",
            "description": "Inspect Git-backed architectural history and drift evidence for a CKB project.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "project_id": { "type": "string", "minLength": 1, "maxLength": 160 },
                    "max_commits": { "type": "integer", "minimum": 1, "maximum": 500, "default": 50 }
                },
                "required": ["project_id"],
                "additionalProperties": false
            },
            "securitySchemes": security(ARCHITECTURE_SCOPE),
            "annotations": read_only_annotations(true)
        }),
        json!({
            "name": "ckb_get_test_gaps",
            "title": "Find test coverage gaps",
            "description": "Use CKB's graph-aware test analysis to identify important architecture paths and nodes that lack test coverage.",
            "inputSchema": project_schema(),
            "securitySchemes": security(ARCHITECTURE_SCOPE),
            "annotations": read_only_annotations(false)
        }),
        json!({
            "name": "ckb_find_causal_path",
            "title": "Find causal architecture path",
            "description": "Find the shortest architecture path between two CKB node IDs, with evidence for how one component can influence another.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "project_id": { "type": "string", "minLength": 1, "maxLength": 160 },
                    "source": { "type": "string", "minLength": 1, "description": "CKB source node ID" },
                    "target": { "type": "string", "minLength": 1, "description": "CKB target node ID" },
                    "max_depth": { "type": "integer", "minimum": 1, "maximum": 64, "default": 12 }
                },
                "required": ["project_id", "source", "target"],
                "additionalProperties": false
            },
            "securitySchemes": security(ARCHITECTURE_SCOPE),
            "annotations": read_only_annotations(false)
        }),
        json!({
            "name": "ckb_get_failure_cone",
            "title": "Trace failure cone",
            "description": "Trace the transitive downstream failure cone from a CKB node ID to show what can be affected if that component fails or changes.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "project_id": { "type": "string", "minLength": 1, "maxLength": 160 },
                    "root": { "type": "string", "minLength": 1, "description": "CKB root node ID" },
                    "max_depth": { "type": "integer", "minimum": 1, "maximum": 64, "default": 12 }
                },
                "required": ["project_id", "root"],
                "additionalProperties": false
            },
            "securitySchemes": security(ARCHITECTURE_SCOPE),
            "annotations": read_only_annotations(false)
        }),
        json!({
            "name": "ckb_query_architecture_memory",
            "title": "Query architecture memory",
            "description": "Ask CKB Architecture Memory for a graph-grounded context slice related to an architectural concept, file, component, or symbol.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "project_id": { "type": "string", "minLength": 1, "maxLength": 160 },
                    "query": { "type": "string", "minLength": 1, "maxLength": 4000 },
                    "depth": { "type": "integer", "minimum": 1, "maximum": 8, "default": 2 },
                    "limit": { "type": "integer", "minimum": 1, "maximum": 100, "default": 12 }
                },
                "required": ["project_id", "query"],
                "additionalProperties": false
            },
            "securitySchemes": security(ARCHITECTURE_SCOPE),
            "annotations": read_only_annotations(false)
        }),
        json!({
            "name": "ckb_get_code_dna",
            "title": "Get code DNA",
            "description": "Return CKB's architecture-memory Code DNA summary for a scanned project.",
            "inputSchema": project_schema(),
            "securitySchemes": security(ARCHITECTURE_SCOPE),
            "annotations": read_only_annotations(false)
        }),
        json!({
            "name": "ckb_list_snapshots",
            "title": "List architecture snapshots",
            "description": "List persistent architecture snapshots captured for a CKB project.",
            "inputSchema": project_schema(),
            "securitySchemes": security(ARCHITECTURE_SCOPE),
            "annotations": read_only_annotations(false)
        }),
        json!({
            "name": "ckb_diff_snapshots",
            "title": "Diff architecture snapshots",
            "description": "Compare two persistent CKB architecture snapshots and return node, edge, and violation deltas.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "project_id": { "type": "string", "minLength": 1, "maxLength": 160 },
                    "from_snapshot": { "type": "string", "minLength": 1 },
                    "to_snapshot": { "type": "string", "minLength": 1 }
                },
                "required": ["project_id", "from_snapshot", "to_snapshot"],
                "additionalProperties": false
            },
            "securitySchemes": security(ARCHITECTURE_SCOPE),
            "annotations": read_only_annotations(false)
        }),
        json!({
            "name": "ckb_generate_ai_rules",
            "title": "Generate architecture guardrails",
            "description": "Generate AI coding guidelines and architecture guardrails from the project's current CKB graph. The rules are returned as text and are not written into the repository.",
            "inputSchema": project_schema(),
            "securitySchemes": security(ARCHITECTURE_SCOPE),
            "annotations": read_only_annotations(false)
        }),
    ]
}
