use axum::{
    body::{to_bytes, Body},
    extract::State,
    http::{header, Request, Response, StatusCode},
    routing::get,
    Json, Router,
};
use reqwest::Client;
use serde_json::json;
use std::{net::SocketAddr, sync::Arc, time::Duration};
use tokio::process::Command;
use tracing::{error, info};

const MAX_PROXY_BODY: usize = 95 * 1024 * 1024;

#[derive(Clone)]
struct StateData {
    client: Client,
    upstream: Arc<String>,
}

async fn upstream_health(state: &StateData) -> Result<reqwest::Response, String> {
    state
        .client
        .get(format!("{}/health", state.upstream))
        .timeout(Duration::from_secs(5))
        .send()
        .await
        .map_err(|error| error.to_string())
}

async fn health(State(state): State<StateData>) -> Response<Body> {
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

async fn ready(State(state): State<StateData>) -> Response<Body> {
    health(State(state)).await
}

async fn proxy(State(state): State<StateData>, request: Request<Body>) -> Response<Body> {
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

    for (name, value) in headers.iter() {
        if name != header::HOST && name != header::CONTENT_LENGTH {
            upstream = upstream.header(name, value);
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

    let status = StatusCode::from_u16(upstream.status().as_u16())
        .unwrap_or(StatusCode::BAD_GATEWAY);
    let content_type = upstream.headers().get(reqwest::header::CONTENT_TYPE).cloned();
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

    let state = StateData {
        client: Client::builder().timeout(Duration::from_secs(300)).build()?,
        upstream: Arc::new(format!("http://127.0.0.1:{gateway_port}")),
    };

    let app = Router::new()
        .route("/health", get(health))
        .route("/ready", get(ready))
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
