use axum::{
    body::{to_bytes, Body},
    extract::State,
    http::{header, HeaderMap, Request, Response, StatusCode},
    response::IntoResponse,
    routing::get,
    Json, Router,
};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use std::{
    collections::HashMap,
    net::SocketAddr,
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};
use tokio::{
    process::Command,
    sync::{RwLock, Semaphore},
};
use tracing::{error, info, warn};

const MAX_PROXY_BODY: usize = 90 * 1024 * 1024;

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ObservedSpan {
    trace_id: String,
    span_id: String,
    parent_span_id: String,
    node_id: String,
    name: String,
    start_unix_nano: u64,
    end_unix_nano: u64,
    duration_ms: f64,
    error: bool,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ObservedTrace {
    trace_id: String,
    first_seen_unix_nano: u64,
    last_seen_unix_nano: u64,
    spans: Vec<ObservedSpan>,
}

#[derive(Clone)]
struct GatewayState {
    client: Client,
    child_base_url: Arc<String>,
    internal_secret: Option<Arc<String>>,
    api_key: Option<Arc<String>>,
    scan_gate: Arc<Semaphore>,
    max_concurrent_scans: usize,
    allow_local_scan: bool,
    data_dir: Arc<PathBuf>,
    traces: Arc<RwLock<HashMap<String, Vec<ObservedTrace>>>>,
    max_traces: usize,
    max_spans_per_trace: usize,
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

fn safe_key(value: &str) -> String {
    value
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() || character == '-' || character == '_' {
                character
            } else {
                '_'
            }
        })
        .collect()
}

fn trace_file(state: &GatewayState, project_key: &str) -> PathBuf {
    state
        .data_dir
        .join("gateway_traces")
        .join(format!("{}.json", safe_key(project_key)))
}

fn decode_hex(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

fn percent_decode(value: &str) -> String {
    let bytes = value.as_bytes();
    let mut output = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        match bytes[index] {
            b'%' if index + 2 < bytes.len() => {
                if let (Some(high), Some(low)) = (decode_hex(bytes[index + 1]), decode_hex(bytes[index + 2])) {
                    output.push((high << 4) | low);
                    index += 3;
                    continue;
                }
                output.push(bytes[index]);
            }
            b'+' => output.push(b' '),
            value => output.push(value),
        }
        index += 1;
    }
    String::from_utf8_lossy(&output).to_string()
}

fn query_param(query: Option<&str>, name: &str) -> Option<String> {
    query?.split('&').find_map(|part| {
        let (key, value) = part.split_once('=').unwrap_or((part, ""));
        if percent_decode(key) == name {
            Some(percent_decode(value))
        } else {
            None
        }
    })
}

fn project_from_query(query: Option<&str>) -> String {
    query_param(query, "project_id")
        .filter(|value| !value.is_empty())
        .or_else(|| query_param(query, "repo").filter(|value| !value.is_empty()))
        .unwrap_or_else(|| "default".into())
}

fn scalar(value: &Value) -> Option<String> {
    value
        .as_str()
        .map(str::to_string)
        .or_else(|| value.get("stringValue").and_then(Value::as_str).map(str::to_string))
        .or_else(|| {
            value.get("intValue").and_then(|item| {
                item.as_str()
                    .map(str::to_string)
                    .or_else(|| item.as_u64().map(|number| number.to_string()))
            })
        })
        .or_else(|| value.get("boolValue").and_then(Value::as_bool).map(|item| item.to_string()))
        .or_else(|| value.get("doubleValue").and_then(Value::as_f64).map(|item| item.to_string()))
}

fn attributes(value: Option<&Value>) -> HashMap<String, String> {
    let mut output = HashMap::new();
    if let Some(array) = value.and_then(Value::as_array) {
        for item in array {
            if let (Some(key), Some(value)) = (
                item.get("key").and_then(Value::as_str),
                item.get("value").and_then(scalar),
            ) {
                output.insert(key.to_string(), value);
            }
        }
    } else if let Some(object) = value.and_then(Value::as_object) {
        for (key, value) in object {
            if let Some(value) = scalar(value) {
                output.insert(key.clone(), value);
            }
        }
    }
    output
}

fn u64_value(value: Option<&Value>) -> u64 {
    value
        .and_then(Value::as_u64)
        .or_else(|| value.and_then(Value::as_str).and_then(|value| value.parse().ok()))
        .unwrap_or(0)
}

fn span_is_error(span: &Value) -> bool {
    let code = span.get("status").and_then(|value| value.get("code"));
    code.and_then(Value::as_u64)
        .map(|value| value == 2)
        .or_else(|| {
            code.and_then(Value::as_str).map(|value| {
                matches!(
                    value.to_ascii_uppercase().as_str(),
                    "2" | "ERROR" | "STATUS_CODE_ERROR"
                )
            })
        })
        .unwrap_or(false)
}

fn canonical_span_id(span: &Value, resource: &HashMap<String, String>) -> String {
    let mut attrs = resource.clone();
    attrs.extend(attributes(span.get("attributes")));
    let file = attrs
        .get("code.file.path")
        .or_else(|| attrs.get("code.filepath"))
        .or_else(|| attrs.get("code.file.name"));
    let function = attrs
        .get("code.function.name")
        .or_else(|| attrs.get("code.function"))
        .or_else(|| attrs.get("function.name"));
    let namespace = attrs
        .get("code.namespace")
        .or_else(|| attrs.get("service.name"));

    match (file, function, namespace) {
        (Some(file), Some(function), _) => format!("{}::{}", file.replace('\\', "/"), function),
        (Some(file), None, _) => format!("{}::file", file.replace('\\', "/")),
        (None, Some(function), Some(namespace)) => format!("{}::{}", namespace, function),
        (None, Some(function), None) => function.clone(),
        (None, None, Some(namespace)) => format!(
            "{}::{}",
            namespace,
            span.get("name").and_then(Value::as_str).unwrap_or("span")
        ),
        _ => span
            .get("name")
            .and_then(Value::as_str)
            .unwrap_or("span")
            .to_string(),
    }
}

fn flatten_otlp_spans(root: &Value) -> Vec<(Value, HashMap<String, String>)> {
    if let Some(array) = root.as_array() {
        return array
            .iter()
            .cloned()
            .map(|span| (span, HashMap::new()))
            .collect();
    }

    let mut output = Vec::new();
    if let Some(resources) = root.get("resourceSpans").and_then(Value::as_array) {
        for resource in resources {
            let resource_attributes = attributes(
                resource
                    .get("resource")
                    .and_then(|value| value.get("attributes")),
            );
            let scopes = resource
                .get("scopeSpans")
                .or_else(|| resource.get("instrumentationLibrarySpans"))
                .and_then(Value::as_array);
            if let Some(scopes) = scopes {
                for scope in scopes {
                    if let Some(spans) = scope.get("spans").and_then(Value::as_array) {
                        for span in spans {
                            output.push((span.clone(), resource_attributes.clone()));
                        }
                    }
                }
            }
        }
    } else if let Some(spans) = root.get("spans").and_then(Value::as_array) {
        for span in spans {
            output.push((span.clone(), HashMap::new()));
        }
    }
    output
}

fn otlp_payload_from_envelope(envelope: &Value) -> Option<Value> {
    if let Some(raw) = envelope
        .get("raw_spans")
        .or_else(|| envelope.get("otlp_json"))
        .and_then(Value::as_str)
    {
        return serde_json::from_str(raw).ok();
    }
    if let Some(payload) = envelope.get("payload") {
        return Some(payload.clone());
    }
    if envelope.get("resourceSpans").is_some() || envelope.get("spans").is_some() || envelope.is_array() {
        return Some(envelope.clone());
    }
    None
}

fn extract_trace_batch(body: &[u8]) -> Option<(String, Vec<ObservedSpan>)> {
    let envelope: Value = serde_json::from_slice(body).ok()?;
    let project_key = envelope
        .get("project_id")
        .and_then(Value::as_str)
        .or_else(|| envelope.get("repo_name").and_then(Value::as_str))
        .unwrap_or("default")
        .to_string();
    let payload = otlp_payload_from_envelope(&envelope)?;
    let mut observed = Vec::new();

    for (span, resource) in flatten_otlp_spans(&payload) {
        let trace_id = span
            .get("traceId")
            .or_else(|| span.get("trace_id"))
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();
        let span_id = span
            .get("spanId")
            .or_else(|| span.get("span_id"))
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();
        if trace_id.is_empty() || span_id.is_empty() {
            continue;
        }
        let parent_span_id = span
            .get("parentSpanId")
            .or_else(|| span.get("parent_span_id"))
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();
        let start = u64_value(
            span.get("startTimeUnixNano")
                .or_else(|| span.get("start_time_unix_nano")),
        );
        let end = u64_value(
            span.get("endTimeUnixNano")
                .or_else(|| span.get("end_time_unix_nano")),
        );
        observed.push(ObservedSpan {
            trace_id,
            span_id,
            parent_span_id,
            node_id: canonical_span_id(&span, &resource),
            name: span
                .get("name")
                .and_then(Value::as_str)
                .unwrap_or("span")
                .to_string(),
            start_unix_nano: start,
            end_unix_nano: end,
            duration_ms: end.saturating_sub(start) as f64 / 1_000_000.0,
            error: span_is_error(&span),
        });
    }
    Some((project_key, observed))
}

async fn ensure_traces_loaded(state: &GatewayState, project_key: &str) {
    if state.traces.read().await.contains_key(project_key) {
        return;
    }
    let loaded = tokio::fs::read(trace_file(state, project_key))
        .await
        .ok()
        .and_then(|bytes| serde_json::from_slice::<Vec<ObservedTrace>>(&bytes).ok())
        .unwrap_or_default();
    state
        .traces
        .write()
        .await
        .entry(project_key.to_string())
        .or_insert(loaded);
}

async fn persist_trace_snapshot(
    state: &GatewayState,
    project_key: &str,
    traces: &[ObservedTrace],
) -> anyhow::Result<()> {
    let path = trace_file(state, project_key);
    if let Some(parent) = path.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }
    let temp = path.with_extension("json.tmp");
    tokio::fs::write(&temp, serde_json::to_vec(traces)?).await?;
    tokio::fs::rename(temp, path).await?;
    Ok(())
}

async fn store_trace_batch(state: &GatewayState, project_key: &str, spans: Vec<ObservedSpan>) {
    if spans.is_empty() {
        return;
    }
    ensure_traces_loaded(state, project_key).await;

    let snapshot = {
        let mut all_projects = state.traces.write().await;
        let traces = all_projects.entry(project_key.to_string()).or_default();

        for span in spans {
            let trace_index = traces
                .iter()
                .position(|trace| trace.trace_id == span.trace_id);
            let index = match trace_index {
                Some(index) => index,
                None => {
                    traces.push(ObservedTrace {
                        trace_id: span.trace_id.clone(),
                        first_seen_unix_nano: span.start_unix_nano,
                        last_seen_unix_nano: span.end_unix_nano,
                        spans: Vec::new(),
                    });
                    traces.len() - 1
                }
            };
            let trace = &mut traces[index];
            trace.first_seen_unix_nano = if trace.first_seen_unix_nano == 0 {
                span.start_unix_nano
            } else if span.start_unix_nano == 0 {
                trace.first_seen_unix_nano
            } else {
                trace.first_seen_unix_nano.min(span.start_unix_nano)
            };
            trace.last_seen_unix_nano = trace.last_seen_unix_nano.max(span.end_unix_nano);

            if let Some(existing) = trace.spans.iter_mut().find(|item| item.span_id == span.span_id) {
                *existing = span;
            } else if trace.spans.len() < state.max_spans_per_trace {
                trace.spans.push(span);
            }
            trace.spans.sort_by(|a, b| {
                a.start_unix_nano
                    .cmp(&b.start_unix_nano)
                    .then_with(|| a.span_id.cmp(&b.span_id))
            });
        }

        traces.sort_by(|a, b| a.last_seen_unix_nano.cmp(&b.last_seen_unix_nano));
        if traces.len() > state.max_traces {
            let excess = traces.len() - state.max_traces;
            traces.drain(0..excess);
        }
        traces.clone()
    };

    if let Err(error) = persist_trace_snapshot(state, project_key, &snapshot).await {
        warn!("Failed to persist exact runtime traces for {}: {}", project_key, error);
    }
}

fn trace_edges(trace: &ObservedTrace) -> Vec<Value> {
    let by_span: HashMap<&str, &ObservedSpan> = trace
        .spans
        .iter()
        .map(|span| (span.span_id.as_str(), span))
        .collect();
    let mut edges = Vec::new();
    for (sequence, span) in trace.spans.iter().enumerate() {
        if span.parent_span_id.is_empty() {
            continue;
        }
        let Some(parent) = by_span.get(span.parent_span_id.as_str()).copied() else {
            continue;
        };
        edges.push(json!({
            "sequence":sequence,
            "traceId":trace.trace_id,
            "spanId":span.span_id,
            "parentSpanId":span.parent_span_id,
            "source":parent.node_id,
            "target":span.node_id,
            "name":span.name,
            "startUnixNano":span.start_unix_nano,
            "endUnixNano":span.end_unix_nano,
            "durationMs":span.duration_ms,
            "invocationCount":1,
            "avgLatencyMs":span.duration_ms,
            "error":span.error,
            "errorRate":if span.error {1.0}else{0.0},
            "lastSeenUnixNano":span.end_unix_nano,
            "kind":"runtime",
            "evidence":[{
                "source":"otlp-span-parent-child",
                "ref":format!("{}:{}->{}",trace.trace_id,parent.span_id,span.span_id)
            }]
        }));
    }
    edges
}

async fn exact_traces_response(state: &GatewayState, project_key: &str) -> Response<Body> {
    ensure_traces_loaded(state, project_key).await;
    let traces = state
        .traces
        .read()
        .await
        .get(project_key)
        .cloned()
        .unwrap_or_default();

    let mut trace_map = Map::new();
    let mut roots = Map::new();
    for trace in &traces {
        trace_map.insert(trace.trace_id.clone(), Value::Array(trace_edges(trace)));
        roots.insert(
            trace.trace_id.clone(),
            Value::Array(
                trace
                    .spans
                    .iter()
                    .filter(|span| span.parent_span_id.is_empty())
                    .map(|span| {
                        json!({
                            "spanId":span.span_id,
                            "node":span.node_id,
                            "name":span.name,
                            "startUnixNano":span.start_unix_nano,
                            "endUnixNano":span.end_unix_nano,
                            "durationMs":span.duration_ms,
                            "error":span.error
                        })
                    })
                    .collect(),
            ),
        );
    }

    json_response(
        StatusCode::OK,
        json!({
            "kind":"runtime",
            "observed":!traces.is_empty(),
            "traceSemantics":"exact-observed-span-instances",
            "replaySafe":true,
            "traces":trace_map,
            "roots":roots,
            "traceCount":traces.len(),
            "retention":{
                "maxTraces":state.max_traces,
                "maxSpansPerTrace":state.max_spans_per_trace
            },
            "evidence":[{"source":"otlp","ref":"persisted-exact-span-instances"}],
            "synthetic":false
        }),
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
                "runtimeTraceSemantics":"exact-observed-span-instances",
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
    let request_query = request.uri().query().map(str::to_string);
    let request_method = request.method().as_str().to_string();

    if request_path == "/api/v1/scan" && !state.allow_local_scan {
        return json_response(
            StatusCode::FORBIDDEN,
            json!({
                "message":"Local filesystem scanning is disabled on the hosted CKB Reality service. Use an authenticated GitHub or ZIP scan.",
                "synthetic":false
            }),
        );
    }

    if request_path == "/api/v1/intelligence/traces" && request_method == "GET" {
        let project_key = project_from_query(request_query.as_deref());
        return exact_traces_response(&state, &project_key).await;
    }

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

    let exact_trace_batch = if matches!(
        request_path.as_str(),
        "/api/v1/otlp" | "/api/v1/intelligence/telemetry/otlp"
    ) {
        extract_trace_batch(&body)
    } else {
        None
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

    if status.is_success() {
        if let Some((project_key, spans)) = exact_trace_batch {
            store_trace_batch(&state, &project_key, spans).await;
        }
    }

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
    let max_traces = std::env::var("CKB_MAX_RUNTIME_TRACES")
        .ok()
        .and_then(|value| value.parse::<usize>().ok())
        .unwrap_or(100)
        .clamp(10, 1000);
    let max_spans_per_trace = std::env::var("CKB_MAX_SPANS_PER_TRACE")
        .ok()
        .and_then(|value| value.parse::<usize>().ok())
        .unwrap_or(1000)
        .clamp(10, 10_000);
    let data_dir = PathBuf::from(
        std::env::var("CKB_REALITY_DATA_DIR")
            .unwrap_or_else(|_| "./ckb_reality_data".into()),
    );
    tokio::fs::create_dir_all(&data_dir).await?;

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
        data_dir: Arc::new(data_dir),
        traces: Arc::new(RwLock::new(HashMap::new())),
        max_traces,
        max_spans_per_trace,
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
