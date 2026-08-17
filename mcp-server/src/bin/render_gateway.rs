use axum::{
    body::{to_bytes, Body},
    extract::State,
    http::{header, HeaderMap, Request, Response, StatusCode},
    routing::get,
    Router,
};
use reqwest::Client;
use serde_json::{json, Value};
use std::{net::SocketAddr, sync::Arc, time::Duration};
use tokio::process::Command;
use tracing::{error, info, warn};

// The canonical CKB Reality tool registry and the provider-neutral adapter
// layer live alongside the inner Reality gateway they call into. They are
// mounted here rather than in a separate edge process so the immediate-bind
// behaviour Render depends on is preserved for every public route.
#[path = "reality_gateway/chatgpt_mcp.rs"]
mod chatgpt_mcp;
#[path = "reality_gateway/universal_gateway.rs"]
mod universal_gateway;

const MAX_PROXY_BODY: usize = 95 * 1024 * 1024;

#[derive(Clone)]
struct GatewayState {
    client: Client,
    upstream: Arc<String>,
    // Alias of `upstream` used by the MCP and Universal Model Gateway modules.
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

/// Server-to-server authorization for the trusted gateway path.
///
/// Unlike the historical edge binary, a missing credential does not abort the
/// process: Render liveness depends on binding the public port immediately, so
/// an unconfigured deployment fails closed per request with 503 instead of
/// refusing to start.
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
            presented_api_key(headers).map(|presented| secure_eq(presented, expected.as_str()))
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

async fn upstream_health(state: &GatewayState) -> Result<reqwest::Response, String> {
    state
        .client
        .get(format!("{}/health", state.upstream))
        .timeout(Duration::from_secs(5))
        .send()
        .await
        .map_err(|error| error.to_string())
}

async fn health(State(state): State<GatewayState>) -> Response<Body> {
    match upstream_health(&state).await {
        Ok(response) if response.status().is_success() => {
            let status = response.status();
            let bytes = response.bytes().await.unwrap_or_default();
            Response::builder()
                .status(StatusCode::from_u16(status.as_u16()).unwrap_or(StatusCode::OK))
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(bytes))
                .unwrap_or_else(|_| Response::new(Body::empty()))
        }
        Ok(response) => Response::builder()
            .status(StatusCode::SERVICE_UNAVAILABLE)
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(
                json!({
                    "status": "starting",
                    "service": "ckb-render-gateway",
                    "upstreamStatus": response.status().as_u16(),
                    "synthetic": false
                })
                .to_string(),
            ))
            .unwrap_or_else(|_| Response::new(Body::empty())),
        Err(reason) => Response::builder()
            .status(StatusCode::SERVICE_UNAVAILABLE)
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(
                json!({
                    "status": "starting",
                    "service": "ckb-render-gateway",
                    "reason": reason,
                    "synthetic": false
                })
                .to_string(),
            ))
            .unwrap_or_else(|_| Response::new(Body::empty())),
    }
}

async fn ready(State(state): State<GatewayState>) -> Response<Body> {
    health(State(state)).await
}

async fn proxy(State(state): State<GatewayState>, request: Request<Body>) -> Response<Body> {
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

    let headers = request.headers().clone();
    let body = match to_bytes(request.into_body(), MAX_PROXY_BODY).await {
        Ok(body) => body,
        Err(error) => {
            return Response::builder()
                .status(StatusCode::PAYLOAD_TOO_LARGE)
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(
                    json!({"message": format!("Request body rejected: {error}"), "synthetic": false})
                        .to_string(),
                ))
                .unwrap_or_else(|_| Response::new(Body::empty()));
        }
    };

    let mut upstream = state
        .client
        .request(method, format!("{}{}", state.upstream, path_and_query))
        .body(body.to_vec());

    // Axum 0.7 and Reqwest 0.11 use different `http` crate major versions.
    // Forward textual header names/values instead of passing their concrete
    // HeaderName/HeaderValue types across that boundary.
    for (name, value) in headers.iter() {
        if name != header::HOST && name != header::CONTENT_LENGTH {
            if let Ok(value) = value.to_str() {
                upstream = upstream.header(name.as_str(), value);
            }
        }
    }

    let upstream = match upstream.send().await {
        Ok(response) => response,
        Err(error) => {
            error!("Render gateway upstream request failed: {}", error);
            return Response::builder()
                .status(StatusCode::BAD_GATEWAY)
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(
                    json!({"message":"CKB Reality is starting or temporarily unavailable","synthetic":false})
                        .to_string(),
                ))
                .unwrap_or_else(|_| Response::new(Body::empty()));
        }
    };

    let status =
        StatusCode::from_u16(upstream.status().as_u16()).unwrap_or(StatusCode::BAD_GATEWAY);
    let content_type = upstream
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .map(str::to_string);
    let bytes = upstream.bytes().await.unwrap_or_default();

    let mut builder = Response::builder().status(status);
    if let Some(value) = content_type {
        builder = builder.header(header::CONTENT_TYPE, value);
    }
    builder
        .body(Body::from(bytes))
        .unwrap_or_else(|_| Response::new(Body::empty()))
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt::init();

    let public_port = std::env::var("PORT")
        .ok()
        .and_then(|value| value.parse::<u16>().ok())
        .unwrap_or(10000);
    let gateway_port = std::env::var("CKB_RENDER_INNER_PORT")
        .ok()
        .and_then(|value| value.parse::<u16>().ok())
        .unwrap_or_else(|| public_port.saturating_add(10));
    let child_port = std::env::var("CKB_REALITY_CHILD_PORT")
        .ok()
        .and_then(|value| value.parse::<u16>().ok())
        .unwrap_or_else(|| gateway_port.saturating_add(1));

    let gateway_bin = std::env::var("CKB_REALITY_GATEWAY_BIN")
        .unwrap_or_else(|_| "./target/release/reality_gateway".into());

    let mut child = Command::new(gateway_bin)
        .env("PORT", gateway_port.to_string())
        .env("CKB_REALITY_CHILD_PORT", child_port.to_string())
        .kill_on_drop(true)
        .spawn()?;

    tokio::spawn(async move {
        match child.wait().await {
            Ok(status) => error!("Inner Reality gateway exited: {}", status),
            Err(error) => error!("Inner Reality gateway wait failed: {}", error),
        }
        std::process::exit(1);
    });

    let upstream = Arc::new(format!("http://127.0.0.1:{gateway_port}"));
    let internal_secret = secret_value("CKB_INTERNAL_SECRET");
    let api_key = secret_value("CKB_API_KEY");

    if internal_secret.is_none() {
        warn!("CKB_INTERNAL_SECRET is not configured. OAuth token introspection and trusted gateway authentication will fail closed.");
    }
    if api_key.is_none() {
        warn!("CKB_API_KEY is not configured. The MCP and Universal Model Gateway routes will fail closed until it is set.");
    }

    let state = GatewayState {
        client: Client::builder()
            .timeout(Duration::from_secs(300))
            .build()?,
        upstream: Arc::clone(&upstream),
        child_base_url: upstream,
        internal_secret,
        api_key,
    };

    let app = Router::new()
        .route("/health", get(health))
        .route("/ready", get(ready))
        // RFC 9728 protected-resource metadata. MCP clients discover the Cloud
        // authorization server from here before presenting a token.
        .route(
            "/.well-known/oauth-protected-resource",
            get(chatgpt_mcp::oauth_protected_resource),
        )
        // Stateless Streamable HTTP MCP endpoint.
        .route(
            "/mcp",
            get(chatgpt_mcp::get_mcp).post(chatgpt_mcp::post_mcp),
        )
        // Universal Model Gateway: provider-shaped views of the one canonical
        // registry. Adapter calls route back through the MCP handler, so they
        // cannot bypass its scope checks.
        .route("/llm/capabilities", get(universal_gateway::capabilities))
        .route("/llm/tools", get(universal_gateway::list_tools))
        .route(
            "/llm/call",
            axum::routing::post(universal_gateway::call_tool),
        )
        // Everything else continues to the inner Reality gateway unchanged.
        .fallback(proxy)
        .with_state(state);

    // Bind immediately so Render detects the web service port while the inner
    // Reality gateway + v5 child finish warming. /health remains 503 until the
    // upstream is healthy, so Render still does not route traffic prematurely.
    let address = SocketAddr::from(([0, 0, 0, 0], public_port));
    info!("CKB Render gateway listening immediately on {}", address);
    let listener = tokio::net::TcpListener::bind(address).await?;
    axum::serve(listener, app).await?;
    Ok(())
}
