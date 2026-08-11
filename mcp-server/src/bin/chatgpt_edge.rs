use axum::{
    body::{to_bytes, Body},
    extract::State,
    http::{header, HeaderMap, Request, Response, StatusCode},
    response::IntoResponse,
    routing::get,
    Router,
};
use reqwest::Client;
use serde_json::{json, Value};
use std::{net::SocketAddr, sync::Arc, time::Duration};
use tokio::process::Command;
use tracing::{error, info, warn};

#[path = "reality_gateway/chatgpt_mcp.rs"]
mod chatgpt_mcp;
#[path = "reality_gateway/universal_gateway.rs"]
mod universal_gateway;

const MAX_PROXY_BODY: usize = 90 * 1024 * 1024;

#[derive(Clone)]
struct GatewayState {
    client: Client,
    child_base_url: Arc<String>,
    internal_secret: Option<Arc<String>>,
    api_key: Option<Arc<String>>,
}

fn secret_value(name: &str) -> Option<Arc<String>> {
    std::env::var(name)
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .map(Arc::new)
}

fn secure_eq(left: &str, right: &str) -> bool {
    let left = left.as_bytes();
    let right = right.as_bytes();
    if left.len() != right.len() {
        return false;
    }
    let mut diff = 0u8;
    for (a, b) in left.iter().zip(right.iter()) {
        diff |= a ^ b;
    }
    diff == 0
}

fn header_text<'a>(headers: &'a HeaderMap, name: &str) -> Option<&'a str> {
    headers.get(name).and_then(|value| value.to_str().ok())
}

fn presented_api_key(headers: &HeaderMap) -> Option<&str> {
    header_text(headers, "x-api-key").or_else(|| {
        header_text(headers, header::AUTHORIZATION.as_str())
            .and_then(|value| value.strip_prefix("Bearer "))
    })
}

fn authorized(state: &GatewayState, headers: &HeaderMap) -> Result<(), (StatusCode, String)> {
    if state.internal_secret.is_none() && state.api_key.is_none() {
        return Err((
            StatusCode::SERVICE_UNAVAILABLE,
            "CKB MCP authentication is not configured on this deployment".into(),
        ));
    }

    let internal_ok = state
        .internal_secret
        .as_ref()
        .and_then(|expected| {
            header_text(headers, "x-ckb-internal-secret")
                .map(|presented| secure_eq(presented, expected.as_str()))
        })
        .unwrap_or(false);
    let api_ok = state
        .api_key
        .as_ref()
        .and_then(|expected| {
            presented_api_key(headers)
                .map(|presented| secure_eq(presented, expected.as_str()))
        })
        .unwrap_or(false);

    if internal_ok || api_ok {
        Ok(())
    } else {
        Err((
            StatusCode::UNAUTHORIZED,
            "Missing or invalid CKB MCP credentials".into(),
        ))
    }
}

fn json_response(status: StatusCode, value: Value) -> Response<Body> {
    Response::builder()
        .status(status)
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(value.to_string()))
        .unwrap_or_else(|_| Response::new(Body::empty()))
}

async fn child_health(state: &GatewayState) -> Result<(), String> {
    let response = state
        .client
        .get(format!("{}/health", state.child_base_url))
        .send()
        .await
        .map_err(|error| error.to_string())?;
    if response.status().is_success() {
        Ok(())
    } else {
        Err(format!("inner gateway health returned {}", response.status()))
    }
}

async fn health(State(state): State<GatewayState>) -> impl IntoResponse {
    match child_health(&state).await {
        Ok(()) => (
            StatusCode::OK,
            axum::Json(json!({
                "status": "ok",
                "service": "ckb-universal-model-edge",
                "mcp": "/mcp",
                "universal_model_gateway": {
                    "capabilities": "/llm/capabilities",
                    "tools": "/llm/tools",
                    "call": "/llm/call"
                },
                "oauth_resource_metadata": "/.well-known/oauth-protected-resource",
                "upstream": "reality_gateway"
            })),
        ),
        Err(error) => (
            StatusCode::SERVICE_UNAVAILABLE,
            axum::Json(json!({
                "status": "degraded",
                "service": "ckb-universal-model-edge",
                "message": error
            })),
        ),
    }
}

async fn proxy(
    State(state): State<GatewayState>,
    request: Request<Body>,
) -> Response<Body> {
    let method = match reqwest::Method::from_bytes(request.method().as_str().as_bytes()) {
        Ok(method) => method,
        Err(_) => {
            return Response::builder()
                .status(StatusCode::METHOD_NOT_ALLOWED)
                .body(Body::empty())
                .unwrap_or_else(|_| Response::new(Body::empty()));
        }
    };
    let path_and_query = request
        .uri()
        .path_and_query()
        .map(|value| value.as_str())
        .unwrap_or(request.uri().path())
        .to_string();

    let forwarded_headers = [
        header::CONTENT_TYPE.as_str(),
        header::ACCEPT.as_str(),
        header::AUTHORIZATION.as_str(),
        "x-api-key",
        "x-ckb-internal-secret",
        "mcp-protocol-version",
        "mcp-session-id",
    ]
    .into_iter()
    .filter_map(|name| {
        request
            .headers()
            .get(name)
            .and_then(|value| value.to_str().ok())
            .map(|value| (name.to_string(), value.to_string()))
    })
    .collect::<Vec<_>>();

    let body = match to_bytes(request.into_body(), MAX_PROXY_BODY).await {
        Ok(body) => body,
        Err(error) => {
            return json_response(
                StatusCode::PAYLOAD_TOO_LARGE,
                json!({ "message": format!("Request body rejected: {error}") }),
            );
        }
    };

    let mut upstream = state
        .client
        .request(method, format!("{}{}", state.child_base_url, path_and_query))
        .body(body.to_vec());
    for (name, value) in forwarded_headers {
        upstream = upstream.header(name, value);
    }

    let upstream = match upstream.send().await {
        Ok(response) => response,
        Err(error) => {
            error!("CKB inner gateway request failed: {}", error);
            return json_response(
                StatusCode::BAD_GATEWAY,
                json!({ "message": "CKB Reality gateway is temporarily unavailable" }),
            );
        }
    };

    let status = StatusCode::from_u16(upstream.status().as_u16())
        .unwrap_or(StatusCode::BAD_GATEWAY);
    let content_type = upstream
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .map(str::to_string);
    let mcp_session_id = upstream
        .headers()
        .get("mcp-session-id")
        .and_then(|value| value.to_str().ok())
        .map(str::to_string);
    let bytes = match upstream.bytes().await {
        Ok(bytes) => bytes,
        Err(error) => {
            error!("CKB inner gateway response body failed: {}", error);
            return Response::builder()
                .status(StatusCode::BAD_GATEWAY)
                .body(Body::empty())
                .unwrap_or_else(|_| Response::new(Body::empty()));
        }
    };

    let mut builder = Response::builder().status(status);
    if let Some(content_type) = content_type {
        builder = builder.header(header::CONTENT_TYPE, content_type);
    }
    if let Some(session_id) = mcp_session_id {
        builder = builder.header("mcp-session-id", session_id);
    }
    builder
        .body(Body::from(bytes))
        .unwrap_or_else(|_| Response::new(Body::empty()))
}

async fn wait_for_child(state: &GatewayState) -> anyhow::Result<()> {
    for _ in 0..120 {
        if child_health(state).await.is_ok() {
            return Ok(());
        }
        tokio::time::sleep(Duration::from_millis(500)).await;
    }
    anyhow::bail!("CKB Reality gateway did not become healthy within 60 seconds")
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt::init();

    let edge_port = std::env::var("PORT")
        .ok()
        .and_then(|value| value.parse::<u16>().ok())
        .unwrap_or(3000);
    let inner_gateway_port = std::env::var("CKB_INNER_GATEWAY_PORT")
        .ok()
        .and_then(|value| value.parse::<u16>().ok())
        .unwrap_or_else(|| edge_port.saturating_add(1));
    let reality_child_port = std::env::var("CKB_REALITY_CHILD_PORT")
        .ok()
        .and_then(|value| value.parse::<u16>().ok())
        .unwrap_or_else(|| inner_gateway_port.saturating_add(1));
    let child_base_url = format!("http://127.0.0.1:{inner_gateway_port}");

    let client = Client::builder()
        .timeout(Duration::from_secs(300))
        .build()?;
    let state = GatewayState {
        client,
        child_base_url: Arc::new(child_base_url),
        internal_secret: secret_value("CKB_INTERNAL_SECRET"),
        api_key: secret_value("CKB_API_KEY"),
    };

    if state.internal_secret.is_none() {
        warn!("CKB_INTERNAL_SECRET is not configured. OAuth token introspection and trusted gateway authentication will fail closed.");
    }
    if state.api_key.is_none() {
        anyhow::bail!("CKB_API_KEY is required by the current production Reality gateway path. It is server-side infrastructure auth only; end users authenticate with OAuth.");
    }

    let executable = std::env::var("CKB_REALITY_GATEWAY_BIN")
        .unwrap_or_else(|_| "./target/release/reality_gateway".into());
    let mut child = Command::new(executable)
        .env("PORT", inner_gateway_port.to_string())
        .env("CKB_REALITY_CHILD_PORT", reality_child_port.to_string())
        .kill_on_drop(true)
        .spawn()?;

    wait_for_child(&state).await?;
    info!("CKB Reality gateway healthy on {}", state.child_base_url);

    tokio::spawn(async move {
        match child.wait().await {
            Ok(status) => error!("CKB Reality gateway exited: {}", status),
            Err(error) => error!("CKB Reality gateway wait failed: {}", error),
        }
        std::process::exit(1);
    });

    let app = Router::new()
        .route("/health", get(health))
        .route(
            "/.well-known/oauth-protected-resource",
            get(chatgpt_mcp::oauth_protected_resource),
        )
        .route(
            "/mcp",
            get(chatgpt_mcp::get_mcp).post(chatgpt_mcp::post_mcp),
        )
        .route("/llm/capabilities", get(universal_gateway::capabilities))
        .route("/llm/tools", get(universal_gateway::list_tools))
        .route(
            "/llm/call",
            axum::routing::post(universal_gateway::call_tool),
        )
        .fallback(proxy)
        .with_state(state);

    let address = SocketAddr::from(([0, 0, 0, 0], edge_port));
    info!("CKB Universal Model Gateway listening on {}", address);
    let listener = tokio::net::TcpListener::bind(address).await?;
    axum::serve(listener, app).await?;
    Ok(())
}
