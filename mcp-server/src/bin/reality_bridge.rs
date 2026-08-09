use axum::{
    extract::State,
    http::StatusCode,
    routing::{get, post},
    Json, Router,
};
use ckb_core::{
    ArchitectureMemoryEngine, CausalArchitectureEngine, CkbEngine, NodeId,
};
use serde::Deserialize;
use serde_json::{json, Value};
use std::{net::SocketAddr, sync::Arc};
use tower_http::cors::{Any, CorsLayer};

#[derive(Clone)]
struct AppState {
    engine: Arc<CkbEngine>,
}

#[derive(Deserialize)]
struct MemoryQueryRequest {
    query: String,
    depth: Option<usize>,
    limit: Option<usize>,
}

#[derive(Deserialize)]
struct CausalPathRequest {
    source: String,
    target: String,
    max_depth: Option<usize>,
}

#[derive(Deserialize)]
struct FailureConeRequest {
    root: String,
    max_depth: Option<usize>,
}

fn internal<E: std::fmt::Display>(error: E) -> (StatusCode, String) {
    (StatusCode::INTERNAL_SERVER_ERROR, error.to_string())
}

fn kind<T: std::fmt::Debug>(value: T) -> String {
    format!("{:?}", value).to_ascii_lowercase()
}

async fn health() -> Json<Value> {
    Json(json!({
        "status": "healthy",
        "service": "ckb-reality-bridge",
        "stage": "stable-core-intelligence",
        "synthetic": false
    }))
}

async fn graph(
    State(state): State<AppState>,
) -> Result<Json<Value>, (StatusCode, String)> {
    let graph = state.engine.architecture_graph_snapshot().await;

    let nodes = graph
        .nodes()
        .into_iter()
        .map(|node| {
            let runtime = graph.get_runtime_metrics(&node.id);
            json!({
                "id": node.id.0,
                "name": node.name,
                "kind": kind(node.kind),
                "path": node.path,
                "line": node.line,
                "column": node.column,
                "metadata": node.metadata,
                "runtime": runtime,
                "intelligence": {
                    "kind": if runtime.is_some() { "runtime" } else { "static" },
                    "confidence": 1.0,
                    "evidence": [{
                        "source": "tree-sitter-ast",
                        "ref": format!("{}:{}:{}", node.path.to_string_lossy(), node.line, node.column)
                    }]
                }
            })
        })
        .collect::<Vec<_>>();

    let links = graph
        .edges()
        .into_iter()
        .map(|edge| {
            json!({
                "id": edge.id,
                "source": edge.from.0,
                "target": edge.to.0,
                "kind": kind(edge.kind),
                "weight": edge.weight,
                "metadata": edge.metadata,
                "intelligence": {
                    "kind": "static",
                    "confidence": 1.0,
                    "evidence": [{
                        "source": "ckb-graph",
                        "ref": format!("{}->{}", edge.from.0, edge.to.0)
                    }]
                }
            })
        })
        .collect::<Vec<_>>();

    Ok(Json(json!({
        "graph": { "nodes": nodes, "links": links },
        "generatedAt": chrono::Utc::now().to_rfc3339(),
        "evidencePolicy": "static-runtime-predicted-separated",
        "synthetic": false
    })))
}

async fn snapshots(
    State(state): State<AppState>,
) -> Result<Json<Value>, (StatusCode, String)> {
    let mut snapshots = state
        .engine
        .architecture_snapshot_metadata()
        .await
        .map_err(internal)?;
    snapshots.sort_by(|a, b| a.timestamp.cmp(&b.timestamp));

    Ok(Json(json!({
        "snapshots": snapshots,
        "source": "ckb-persistent-sled-snapshots",
        "synthetic": false
    })))
}

async fn code_dna(
    State(state): State<AppState>,
) -> Result<Json<Value>, (StatusCode, String)> {
    let graph = state.engine.architecture_graph_snapshot().await;
    let report = ArchitectureMemoryEngine::code_dna(&graph).map_err(internal)?;
    Ok(Json(serde_json::to_value(report).map_err(internal)?))
}

async fn memory_query(
    State(state): State<AppState>,
    Json(request): Json<MemoryQueryRequest>,
) -> Result<Json<Value>, (StatusCode, String)> {
    let graph = state.engine.architecture_graph_snapshot().await;
    let report = ArchitectureMemoryEngine::query(
        &graph,
        &request.query,
        request.depth.unwrap_or(2),
        request.limit.unwrap_or(12),
    )
    .map_err(internal)?;
    Ok(Json(serde_json::to_value(report).map_err(internal)?))
}

async fn causal_path(
    State(state): State<AppState>,
    Json(request): Json<CausalPathRequest>,
) -> Result<Json<Value>, (StatusCode, String)> {
    let graph = state.engine.architecture_graph_snapshot().await;
    let report = CausalArchitectureEngine::shortest_path(
        &graph,
        &NodeId(request.source),
        &NodeId(request.target),
        request.max_depth.unwrap_or(12),
    )
    .map_err(internal)?;
    Ok(Json(serde_json::to_value(report).map_err(internal)?))
}

async fn failure_cone(
    State(state): State<AppState>,
    Json(request): Json<FailureConeRequest>,
) -> Result<Json<Value>, (StatusCode, String)> {
    let graph = state.engine.architecture_graph_snapshot().await;
    let report = CausalArchitectureEngine::failure_cone(
        &graph,
        &NodeId(request.root),
        request.max_depth.unwrap_or(12),
    )
    .map_err(internal)?;
    Ok(Json(serde_json::to_value(report).map_err(internal)?))
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt::init();
    let state = AppState {
        engine: Arc::new(CkbEngine::new()?),
    };

    let app = Router::new()
        .route("/health", get(health))
        .route("/api/v1/intelligence/graph", get(graph))
        .route("/api/v1/intelligence/snapshots", get(snapshots))
        .route("/api/v1/intelligence/code-dna", get(code_dna))
        .route("/api/v1/intelligence/memory/query", post(memory_query))
        .route("/api/v1/intelligence/causal-path", post(causal_path))
        .route("/api/v1/intelligence/failure-cone", post(failure_cone))
        .layer(
            CorsLayer::new()
                .allow_origin(Any)
                .allow_methods(Any)
                .allow_headers(Any),
        )
        .with_state(state);

    let port: u16 = std::env::var("PORT")
        .ok()
        .and_then(|value| value.parse::<u16>().ok())
        .unwrap_or(3000);
    let address = SocketAddr::from(([0, 0, 0, 0], port));
    let listener = tokio::net::TcpListener::bind(address).await?;
    axum::serve(listener, app).await?;
    Ok(())
}
