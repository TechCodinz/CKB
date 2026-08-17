use super::{chatgpt_mcp, json_response, GatewayState};
use axum::{
    body::{to_bytes, Body},
    extract::{Query, State},
    http::{header, HeaderMap, Response, StatusCode},
    Json,
};
use serde::Deserialize;
use serde_json::{json, Value};

const MAX_MCP_ADAPTER_RESPONSE: usize = 16 * 1024 * 1024;

#[derive(Debug, Default, Deserialize)]
pub(super) struct ToolQuery {
    provider: Option<String>,
}

#[derive(Debug, Deserialize)]
pub(super) struct UniversalToolCall {
    name: String,
    #[serde(default)]
    arguments: Value,
    provider: Option<String>,
}

pub(super) async fn capabilities() -> Response<Body> {
    json_response(
        StatusCode::OK,
        json!({
            "service": "CKB Universal Model Gateway",
            "version": 1,
            "architecture_engine": "CKB Reality",
            "native_mcp": {
                "transport": "streamable-http",
                "endpoint": "/mcp",
                "clients": [
                    "OpenAI ChatGPT/Codex MCP clients",
                    "Anthropic Claude/Claude Code MCP clients",
                    "xAI Grok MCP clients",
                    "Google Gemini remote-MCP clients",
                    "other standards-compatible MCP clients"
                ]
            },
            "function_tool_adapter": {
                "tools_endpoint": "/llm/tools?provider=<provider>",
                "call_endpoint": "/llm/call",
                "providers": [
                    "openai",
                    "deepseek",
                    "xai",
                    "anthropic",
                    "gemini",
                    "generic",
                    "mcp"
                ]
            },
            "authentication": {
                "oauth": true,
                "oauth_resource_metadata": "/.well-known/oauth-protected-resource",
                "operator_api_key": true,
                "target_repository_write_access": false
            },
            "tool_count": 13,
            "evidence_model": [
                "static-architecture",
                "runtime-telemetry",
                "git-history",
                "architecture-memory"
            ]
        }),
    )
}

pub(super) async fn list_tools(
    State(state): State<GatewayState>,
    Query(query): Query<ToolQuery>,
) -> Response<Body> {
    let provider = normalize_provider(query.provider.as_deref());
    let envelope = match invoke_mcp(
        state,
        HeaderMap::new(),
        json!({
            "jsonrpc": "2.0",
            "id": "universal-tools",
            "method": "tools/list",
            "params": {}
        }),
    )
    .await
    {
        Ok(value) => value,
        Err(message) => {
            return json_response(
                StatusCode::BAD_GATEWAY,
                json!({ "ok": false, "error": "mcp_discovery_failed", "message": message }),
            )
        }
    };

    if let Some(error) = envelope.get("error") {
        return json_response(
            StatusCode::BAD_GATEWAY,
            json!({ "ok": false, "error": "mcp_discovery_failed", "details": error }),
        );
    }

    let tools = envelope
        .pointer("/result/tools")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();

    json_response(StatusCode::OK, format_tools(&provider, &tools))
}

pub(super) async fn call_tool(
    State(state): State<GatewayState>,
    headers: HeaderMap,
    Json(call): Json<UniversalToolCall>,
) -> Response<Body> {
    let name = call.name.trim();
    if name.is_empty() {
        return json_response(
            StatusCode::BAD_REQUEST,
            json!({ "ok": false, "error": "invalid_tool_name", "message": "name is required" }),
        );
    }

    let arguments = if call.arguments.is_null() {
        json!({})
    } else if call.arguments.is_object() {
        call.arguments
    } else {
        return json_response(
            StatusCode::BAD_REQUEST,
            json!({
                "ok": false,
                "error": "invalid_arguments",
                "message": "arguments must be a JSON object"
            }),
        );
    };

    let provider = normalize_provider(call.provider.as_deref());
    let envelope = match invoke_mcp(
        state,
        headers,
        json!({
            "jsonrpc": "2.0",
            "id": "universal-call",
            "method": "tools/call",
            "params": {
                "name": name,
                "arguments": arguments
            }
        }),
    )
    .await
    {
        Ok(value) => value,
        Err(message) => {
            return json_response(
                StatusCode::BAD_GATEWAY,
                json!({ "ok": false, "error": "mcp_execution_failed", "message": message }),
            )
        }
    };

    if let Some(error) = envelope.get("error") {
        return json_response(
            StatusCode::BAD_REQUEST,
            json!({
                "ok": false,
                "provider": provider,
                "tool": name,
                "error": "invalid_tool_call",
                "details": error
            }),
        );
    }

    let result = envelope.get("result").cloned().unwrap_or_else(|| json!({}));
    if result.get("isError").and_then(Value::as_bool) == Some(true) {
        if let Some(challenge) = result
            .pointer("/_meta/mcp~1www_authenticate/0")
            .and_then(Value::as_str)
        {
            let body = json!({
                "ok": false,
                "provider": provider,
                "tool": name,
                "error": "authentication_required",
                "message": result.pointer("/content/0/text").and_then(Value::as_str),
                "oauth_resource_metadata": "/.well-known/oauth-protected-resource"
            });
            return Response::builder()
                .status(StatusCode::UNAUTHORIZED)
                .header(header::CONTENT_TYPE, "application/json")
                .header(header::WWW_AUTHENTICATE, challenge)
                .body(Body::from(body.to_string()))
                .unwrap_or_else(|_| Response::new(Body::empty()));
        }

        return json_response(
            StatusCode::UNPROCESSABLE_ENTITY,
            json!({
                "ok": false,
                "provider": provider,
                "tool": name,
                "error": "tool_execution_error",
                "message": result.pointer("/content/0/text").and_then(Value::as_str),
                "mcp_result": result
            }),
        );
    }

    let output = result
        .get("structuredContent")
        .cloned()
        .or_else(|| result.pointer("/content/0/text").cloned())
        .unwrap_or(Value::Null);

    json_response(
        StatusCode::OK,
        json!({
            "ok": true,
            "provider": provider,
            "tool": name,
            "result": output,
            "mcp_result": result
        }),
    )
}

async fn invoke_mcp(
    state: GatewayState,
    headers: HeaderMap,
    payload: Value,
) -> Result<Value, String> {
    let response = chatgpt_mcp::post_mcp(State(state), headers, Json(payload)).await;
    let status = response.status();
    let bytes = to_bytes(response.into_body(), MAX_MCP_ADAPTER_RESPONSE)
        .await
        .map_err(|error| format!("Unable to read MCP response: {error}"))?;
    if !status.is_success() {
        return Err(format!(
            "MCP transport returned HTTP {status}: {}",
            String::from_utf8_lossy(&bytes)
        ));
    }
    serde_json::from_slice(&bytes)
        .map_err(|error| format!("MCP transport returned invalid JSON: {error}"))
}

fn normalize_provider(provider: Option<&str>) -> String {
    match provider
        .unwrap_or("generic")
        .trim()
        .to_ascii_lowercase()
        .as_str()
    {
        "chatgpt" | "codex" | "openai" => "openai".into(),
        "deepseek" => "deepseek".into(),
        "grok" | "xai" => "xai".into(),
        "claude" | "claude-code" | "anthropic" => "anthropic".into(),
        "google" | "gemini" => "gemini".into(),
        "mcp" => "mcp".into(),
        _ => "generic".into(),
    }
}

fn format_tools(provider: &str, tools: &[Value]) -> Value {
    match provider {
        "openai" | "deepseek" | "xai" => json!({
            "provider": provider,
            "format": "openai-compatible-function-tools",
            "native_mcp_recommended": provider != "deepseek",
            "tools": tools.iter().map(openai_tool).collect::<Vec<_>>()
        }),
        "anthropic" => json!({
            "provider": provider,
            "format": "anthropic-tools",
            "native_mcp_recommended": true,
            "tools": tools.iter().map(anthropic_tool).collect::<Vec<_>>()
        }),
        "gemini" => json!({
            "provider": provider,
            "format": "gemini-interactions-functions",
            "native_mcp_recommended": true,
            "tools": tools.iter().map(gemini_tool).collect::<Vec<_>>()
        }),
        "mcp" => json!({
            "provider": provider,
            "format": "mcp-tools-list",
            "native_mcp_recommended": true,
            "tools": tools
        }),
        _ => json!({
            "provider": "generic",
            "format": "json-schema-functions",
            "native_mcp_recommended": false,
            "tools": tools.iter().map(generic_tool).collect::<Vec<_>>()
        }),
    }
}

fn openai_tool(tool: &Value) -> Value {
    json!({
        "type": "function",
        "function": {
            "name": tool.get("name").cloned().unwrap_or(Value::Null),
            "description": tool_description(tool),
            "parameters": input_schema(tool),
            "strict": false
        }
    })
}

fn anthropic_tool(tool: &Value) -> Value {
    json!({
        "name": tool.get("name").cloned().unwrap_or(Value::Null),
        "description": tool_description(tool),
        "input_schema": input_schema(tool)
    })
}

fn gemini_tool(tool: &Value) -> Value {
    json!({
        "type": "function",
        "name": tool.get("name").cloned().unwrap_or(Value::Null),
        "description": tool_description(tool),
        "parameters": input_schema(tool)
    })
}

fn generic_tool(tool: &Value) -> Value {
    json!({
        "name": tool.get("name").cloned().unwrap_or(Value::Null),
        "title": tool.get("title").cloned().unwrap_or(Value::Null),
        "description": tool_description(tool),
        "input_schema": input_schema(tool),
        "security": tool.get("securitySchemes").cloned().unwrap_or_else(|| json!([])),
        "annotations": tool.get("annotations").cloned().unwrap_or_else(|| json!({}))
    })
}

fn tool_description(tool: &Value) -> Value {
    tool.get("description")
        .cloned()
        .or_else(|| tool.get("title").cloned())
        .unwrap_or(Value::String("CKB architecture intelligence tool".into()))
}

fn input_schema(tool: &Value) -> Value {
    tool.get("inputSchema")
        .cloned()
        .unwrap_or_else(|| json!({ "type": "object", "properties": {} }))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Client aliases must collapse onto the provider wire format they
    /// actually use, so "codex" and "chatgpt" cannot silently fall through to
    /// the generic shape.
    #[test]
    fn provider_aliases_resolve_to_wire_formats() {
        for alias in ["chatgpt", "codex", "openai", "OpenAI", "  OPENAI  "] {
            assert_eq!(normalize_provider(Some(alias)), "openai", "alias {alias}");
        }
        assert_eq!(normalize_provider(Some("claude-code")), "anthropic");
        assert_eq!(normalize_provider(Some("claude")), "anthropic");
        assert_eq!(normalize_provider(Some("grok")), "xai");
        assert_eq!(normalize_provider(Some("xai")), "xai");
        assert_eq!(normalize_provider(Some("gemini")), "gemini");
        assert_eq!(normalize_provider(Some("google")), "gemini");
        assert_eq!(normalize_provider(Some("deepseek")), "deepseek");
        assert_eq!(normalize_provider(Some("mcp")), "mcp");
    }

    #[test]
    fn unknown_and_missing_providers_fall_back_to_generic() {
        assert_eq!(normalize_provider(None), "generic");
        assert_eq!(normalize_provider(Some("some-future-model")), "generic");
        assert_eq!(normalize_provider(Some("")), "generic");
    }

    fn sample_tools() -> Vec<Value> {
        vec![json!({
            "name": "ckb_get_architecture_graph",
            "title": "Get architecture graph",
            "description": "Return CKB's persisted static architecture graph.",
            "inputSchema": {
                "type": "object",
                "properties": { "project_id": { "type": "string" } },
                "required": ["project_id"],
                "additionalProperties": false
            }
        })]
    }

    /// Every provider view is generated from the one canonical registry, so
    /// each shape must carry the same tool name through its own field layout.
    #[test]
    fn every_provider_shape_preserves_the_canonical_tool_name() {
        let tools = sample_tools();

        let openai = format_tools("openai", &tools);
        assert_eq!(openai["format"], "openai-compatible-function-tools");
        assert_eq!(
            openai["tools"][0]["function"]["name"],
            "ckb_get_architecture_graph"
        );

        let anthropic = format_tools("anthropic", &tools);
        assert_eq!(anthropic["format"], "anthropic-tools");
        assert_eq!(anthropic["tools"][0]["name"], "ckb_get_architecture_graph");

        let gemini = format_tools("gemini", &tools);
        assert_eq!(gemini["format"], "gemini-interactions-functions");
        assert_eq!(gemini["tools"][0]["name"], "ckb_get_architecture_graph");

        let generic = format_tools("generic", &tools);
        assert_eq!(generic["format"], "json-schema-functions");
        assert_eq!(generic["tools"][0]["name"], "ckb_get_architecture_graph");
    }

    /// The `mcp` view is the canonical registry verbatim — no reshaping.
    #[test]
    fn mcp_shape_passes_the_registry_through_unchanged() {
        let tools = sample_tools();
        let shaped = format_tools("mcp", &tools);
        assert_eq!(shaped["format"], "mcp-tools-list");
        assert_eq!(shaped["tools"], json!(tools));
    }

    #[test]
    fn openai_compatible_providers_share_one_shape() {
        let tools = sample_tools();
        for provider in ["openai", "deepseek", "xai"] {
            let shaped = format_tools(provider, &tools);
            assert_eq!(
                shaped["format"], "openai-compatible-function-tools",
                "{provider}"
            );
            assert_eq!(shaped["provider"], provider);
            assert_eq!(shaped["tools"][0]["type"], "function");
        }
    }

    /// An unrecognised provider must still receive a usable schema rather than
    /// an empty or error response.
    #[test]
    fn unknown_provider_still_receives_the_generic_schema() {
        let shaped = format_tools("some-future-model", &sample_tools());
        assert_eq!(shaped["provider"], "generic");
        assert_eq!(shaped["tools"][0]["input_schema"]["type"], "object");
    }

    /// The provider-shaped schema must carry the canonical input schema, not a
    /// hand-maintained copy that could drift from the registry.
    #[test]
    fn provider_schemas_are_derived_from_the_canonical_input_schema() {
        let tools = sample_tools();
        let canonical = &tools[0]["inputSchema"];
        assert_eq!(
            &format_tools("openai", &tools)["tools"][0]["function"]["parameters"],
            canonical
        );
        assert_eq!(
            &format_tools("anthropic", &tools)["tools"][0]["input_schema"],
            canonical
        );
        assert_eq!(
            &format_tools("gemini", &tools)["tools"][0]["parameters"],
            canonical
        );
        assert_eq!(
            &format_tools("generic", &tools)["tools"][0]["input_schema"],
            canonical
        );
    }
}
