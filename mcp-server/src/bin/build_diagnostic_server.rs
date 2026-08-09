use axum::{routing::get, Json, Router};
use serde_json::{json, Value};

async fn health() -> Json<Value> {
    Json(json!({
        "status": "healthy",
        "service": "ckb-build-diagnostic",
        "temporary": true
    }))
}

async fn diagnostics() -> String {
    tokio::fs::read_to_string("reality_v5_build.log")
        .await
        .unwrap_or_else(|error| format!("No Reality v5 compiler log was produced: {error}"))
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let app = Router::new()
        .route("/health", get(health))
        .route("/__ckb_build_diagnostics_v5", get(diagnostics));
    let port = std::env::var("PORT")
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(3000);
    let listener = tokio::net::TcpListener::bind(([0, 0, 0, 0], port)).await?;
    axum::serve(listener, app).await?;
    Ok(())
}
