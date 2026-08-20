use axum::{
    extract::{Path, State},
    http::{header, HeaderMap, StatusCode},
    routing::{get, post},
    Json, Router,
};
use ckb_core::FrontierModelRegistry;
use serde::Deserialize;
use serde_json::{json, Value};
use std::{net::SocketAddr, sync::Arc};
use tower_http::cors::CorsLayer;
use tracing::info;

#[derive(Clone)]
struct AppState {
    registry: Arc<FrontierModelRegistry>,
    api_key: Option<Arc<String>>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct AdaptRequest {
    provider: String,
    model: String,
    request: Value,
}

fn configured_api_key() -> Option<Arc<String>> {
    std::env::var("CKB_API_KEY")
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

fn presented_api_key(headers: &HeaderMap) -> Option<&str> {
    headers
        .get("x-api-key")
        .and_then(|value| value.to_str().ok())
        .or_else(|| {
            headers
                .get(header::AUTHORIZATION)
                .and_then(|value| value.to_str().ok())
                .and_then(|value| value.strip_prefix("Bearer "))
        })
}

fn authorize_mutation(state: &AppState, headers: &HeaderMap) -> Result<(), (StatusCode, Json<Value>)> {
    let Some(expected) = &state.api_key else {
        return Err((
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({
                "error": "model registry adaptation endpoint is not configured for authenticated use",
                "synthetic": false
            })),
        ));
    };

    let valid = presented_api_key(headers)
        .map(|presented| secure_eq(presented, expected.as_str()))
        .unwrap_or(false);

    if valid {
        Ok(())
    } else {
        Err((
            StatusCode::UNAUTHORIZED,
            Json(json!({"error":"missing or invalid CKB API key","synthetic":false})),
        ))
    }
}

async fn health(State(state): State<AppState>) -> Json<Value> {
    Json(json!({
        "status": "ok",
        "service": "ckb-frontier-model-registry",
        "profiles": state.registry.profiles().len(),
        "synthetic": false
    }))
}

async fn list_models(State(state): State<AppState>) -> Json<Value> {
    Json(json!({
        "profiles": state.registry.profiles(),
        "freshness": state.registry.freshness(chrono::Utc::now()).into_iter().map(|entry| json!({
            "provider": entry.provider,
            "model": entry.model,
            "verifiedAt": entry.verified_at,
            "staleAfterDays": entry.stale_after_days,
            "stale": entry.stale
        })).collect::<Vec<_>>(),
        "resolutionPolicy": "exact-model-or-declared-alias-only",
        "synthetic": false
    }))
}

async fn get_model(
    State(state): State<AppState>,
    Path((provider, model)): Path<(String, String)>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let Some(profile) = state.registry.resolve(&provider, &model) else {
        return Err((
            StatusCode::NOT_FOUND,
            Json(json!({
                "error": format!("no verified frontier-model profile for {provider}/{model}"),
                "synthetic": false
            })),
        ));
    };

    Ok(Json(json!({"profile": profile, "synthetic": false})))
}

async fn adapt(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(input): Json<AdaptRequest>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    authorize_mutation(&state, &headers)?;

    match state.registry.adapt_request(&input.provider, &input.model, &input.request) {
        Ok(result) => Ok(Json(json!({"compatibility": result, "synthetic": false}))),
        Err(error) => Err((
            StatusCode::NOT_FOUND,
            Json(json!({"error": error.to_string(), "synthetic": false})),
        )),
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt::init();

    let registry = Arc::new(FrontierModelRegistry::builtin()?);
    let state = AppState {
        registry,
        api_key: configured_api_key(),
    };

    let app = Router::new()
        .route("/health", get(health))
        .route("/api/v1/models", get(list_models))
        .route("/api/v1/models/:provider/:model", get(get_model))
        .route("/api/v1/models/adapt", post(adapt))
        .layer(CorsLayer::permissive())
        .with_state(state);

    let port = std::env::var("PORT")
        .ok()
        .and_then(|value| value.parse::<u16>().ok())
        .unwrap_or(10000);
    let address = SocketAddr::from(([0, 0, 0, 0], port));
    info!("CKB frontier-model registry API listening on {}", address);
    let listener = tokio::net::TcpListener::bind(address).await?;
    axum::serve(listener, app).await?;
    Ok(())
}
