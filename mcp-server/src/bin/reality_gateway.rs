use axum::{
    body::{to_bytes, Body},
    extract::State,
    http::{header, HeaderMap, Request, Response, StatusCode},
    response::IntoResponse,
    routing::get,
    Json, Router,
};
use reqwest::Client;
use serde_json::{json, Value};
use std::{net::SocketAddr, sync::Arc, time::Duration};
use tokio::{process::Command, sync::Semaphore};
use tracing::{error, info, warn};

const MAX_PROXY_BODY: usize = 90 * 1024 * 1024;

#[derive(Clone)]
struct GatewayState {
    client: Client,
    child_base_url: Arc<String>,
    internal_secret: Option<Arc<String>>,
    api_key: Option<Arc<String>>,
    scan_gate: Arc<Semaphore>,
    max_concurrent_scans: usize,
    allow_local_scan: bool,
}

fn secret_value(name: &str) -> Option<Arc<String>> {
    std::env::var(name)
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .map(Arc::new)
}

fn env_flag(name: &str, default: bool) -> bool {
    std::env::var(name)
        .map(|value| value == "1" || value.eq_ignore_ascii_case("true"))
        .unwrap_or(default)
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
    let internal_configured = state.internal_secret.is_some();
    let api_key_configured = state.api_key.is_some();

    if !internal_configured && !api_key_configured {
        return Err((
            StatusCode::SERVICE_UNAVAILABLE,
            "CKB Reality authentication is not configured on this deployment".into(),
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
            "Missing or invalid CKB Reality credentials".into(),
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

fn is_expensive_scan(path: &str) -> bool {
    matches!(
        path,
        "/api/v1/scan"
            | "/api/v1/intelligence/scan/github"
            | "/api/v1/intelligence/scan/zip"
    )
}

async fn child_health(state: &GatewayState) -> Result<Value, String> {
    let response = state
        .client
        .get(format!("{}/health", state.child_base_url))
        .timeout(Duration::from_secs(5))
        .send()
        .await
        .map_err(|error| error.to_string())?;

    if !response.status().is_success() {
        return Err(format!("Reality v5 returned {}", response.status()));
    }

    response
        .json::<Value>()
        .await
        .map_err(|error| error.to_string())
}

async fn health(State(state): State<GatewayState>) -> impl IntoResponse {
    match child_health(&state).await {
        Ok(child) => (
            StatusCode::OK,
            Json(json!({
                "status":"healthy",
                "service":"ckb-reality-gateway",
                "engine":"reality-server-v5",
                "tenantIsolation":"project-session",
                "evidencePolicy":"static-runtime-predicted-separated",
                "authConfigured":state.internal_secret.is_some() || state.api_key.is_some(),
                "localFilesystemScanEnabled":state.allow_local_scan,
                "maxConcurrentScans":state.max_concurrent_scans,
                "child":child
            })),
        ),
        Err(reason) => (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({
                "status":"degraded",
                "service":"ckb-reality-gateway",
                "engine":"reality-server-v5",
                "reason":reason,
                "synthetic":false
            })),
        ),
    }
}

async fn proxy(
    State(state): State<GatewayState>,
    request: Request<Body>,
) -> Response<Body> {
    if let Err((status, message)) = authorized(&state, request.headers()) {
        return json_response(status, json!({"message":message,"synthetic":false}));
    }

    let request_path = request.uri().path().to_string();
    if request_path == "/api/v1/scan" && !state.allow_local_scan {
        return json_response(
            StatusCode::FORBIDDEN,
            json!({
                "message":"Local filesystem scanning is disabled on the hosted CKB Reality service. Use an authenticated GitHub or ZIP scan.",
                "synthetic":false
            }),
        );
    }

    // Repository parsing is CPU/memory/archive intensive. Serialize it by
    // default so a burst of scan requests cannot turn a healthy API into a
    // denial-of-wallet workload. Read-only graph/memory queries stay parallel.
    let _scan_permit = if is_expensive_scan(&request_path) {
        match state.scan_gate.clone().acquire_owned().await {
            Ok(permit) => Some(permit),
            Err(_) => {
                return json_response(
                    StatusCode::SERVICE_UNAVAILABLE,
                    json!({"message":"CKB scan gate is unavailable","synthetic":false}),
                );
            }
        }
    } else {
        None
    };

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
    let content_type = request
        .headers()
        .get(header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .map(str::to_string);
    let accept = request
        .headers()
        .get(header::ACCEPT)
        .and_then(|value| value.to_str().ok())
        .map(str::to_string);

    let body = match to_bytes(request.into_body(), MAX_PROXY_BODY).await {
        Ok(body) => body,
        Err(error) => {
            return json_response(
                StatusCode::PAYLOAD_TOO_LARGE,
                json!({"message":format!("Request body rejected: {error}"),"synthetic":false}),
            );
        }
    };

    let mut upstream = state
        .client
        .request(method, format!("{}{}", state.child_base_url, path_and_query))
        .body(body.to_vec());

    if let Some(value) = content_type {
        upstream = upstream.header(reqwest::header::CONTENT_TYPE, value);
    }
    if let Some(value) = accept {
        upstream = upstream.header(reqwest::header::ACCEPT, value);
    }
    // If the child itself is configured with a shared API key, satisfy that
    // private localhost hop here. Incoming browser/API credentials are never
    // blindly forwarded to the child process.
    if let Some(api_key) = &state.api_key {
        upstream = upstream.header("x-api-key", api_key.as_str());
    }

    let upstream = match upstream.send().await {
        Ok(response) => response,
        Err(error) => {
            error!("Reality v5 upstream request failed: {}", error);
            return json_response(
                StatusCode::BAD_GATEWAY,
                json!({
                    "message":"CKB Reality v5 is temporarily unavailable",
                    "synthetic":false
                }),
            );
        }
    };

    let status = StatusCode::from_u16(upstream.status().as_u16())
        .unwrap_or(StatusCode::BAD_GATEWAY);
    let response_content_type = upstream
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .map(str::to_string);
    let bytes = match upstream.bytes().await {
        Ok(bytes) => bytes,
        Err(error) => {
            error!("Reality v5 upstream body failed: {}", error);
            return Response::builder()
                .status(StatusCode::BAD_GATEWAY)
                .body(Body::empty())
                .unwrap_or_else(|_| Response::new(Body::empty()));
        }
    };

    let mut builder = Response::builder().status(status);
    if let Some(value) = response_content_type {
        builder = builder.header(header::CONTENT_TYPE, value);
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
    anyhow::bail!("Reality v5 child did not become healthy within 60 seconds")
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt::init();

    let gateway_port = std::env::var("PORT")
        .ok()
        .and_then(|value| value.parse::<u16>().ok())
        .unwrap_or(3000);
    let child_port = std::env::var("CKB_REALITY_CHILD_PORT")
        .ok()
        .and_then(|value| value.parse::<u16>().ok())
        .unwrap_or_else(|| if gateway_port == u16::MAX { 3001 } else { gateway_port + 1 });
    let child_base_url = format!("http://127.0.0.1:{child_port}");
    let max_concurrent_scans = std::env::var("CKB_MAX_CONCURRENT_SCANS")
        .ok()
        .and_then(|value| value.parse::<usize>().ok())
        .unwrap_or(1)
        .clamp(1, 4);

    let client = Client::builder()
        .timeout(Duration::from_secs(300))
        .build()?;
    let state = GatewayState {
        client,
        child_base_url: Arc::new(child_base_url),
        internal_secret: secret_value("CKB_INTERNAL_SECRET"),
        api_key: secret_value("CKB_API_KEY"),
        scan_gate: Arc::new(Semaphore::new(max_concurrent_scans)),
        max_concurrent_scans,
        allow_local_scan: env_flag("CKB_ALLOW_LOCAL_SCAN", false),
    };

    if state.internal_secret.is_none() && state.api_key.is_none() {
        warn!("No Reality credentials configured: health will stay public, protected routes will fail closed with 503");
    }

    let executable = std::env::var("CKB_REALITY_V5_BIN")
        .unwrap_or_else(|_| "./target/release/reality_server_v5".into());
    let mut child = Command::new(executable)
        .env("PORT", child_port.to_string())
        .env("CKB_BIND_ALL", "0")
        .kill_on_drop(true)
        .spawn()?;

    wait_for_child(&state).await?;
    info!("CKB Reality v5 child healthy on {}", state.child_base_url);

    tokio::spawn(async move {
        match child.wait().await {
            Ok(status) => error!("Reality v5 child exited: {}", status),
            Err(error) => error!("Reality v5 child wait failed: {}", error),
        }
        std::process::exit(1);
    });

    let app = Router::new()
        .route("/health", get(health))
        .fallback(proxy)
        .with_state(state);

    let address = SocketAddr::from(([0, 0, 0, 0], gateway_port));
    info!("CKB Reality gateway listening on {}", address);
    let listener = tokio::net::TcpListener::bind(address).await?;
    axum::serve(listener, app).await?;
    Ok(())
}
