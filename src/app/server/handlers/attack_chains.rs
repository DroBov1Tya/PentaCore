use crate::app::AppState;
use crate::app::database::queries::attack_chains as db;
use axum::{
    Json,
    extract::{Path, State},
};
use std::sync::Arc;

pub async fn list(
    State(state): State<Arc<AppState>>,
    Path(domain): Path<String>,
) -> Json<serde_json::Value> {
    match db::list(&state.db, &domain).await {
        Ok(rows) => Json(serde_json::json!(rows)),
        Err(e) => Json(serde_json::json!({ "error": e.to_string() })),
    }
}

pub async fn create(
    State(state): State<Arc<AppState>>,
    Path(domain): Path<String>,
    Json(input): Json<db::CreateChain>,
) -> Json<serde_json::Value> {
    match db::create(&state.db, &domain, &input).await {
        Ok(id) => Json(serde_json::json!({ "ok": true, "id": id })),
        Err(e) => Json(serde_json::json!({ "error": e.to_string() })),
    }
}

pub async fn get_steps(
    State(state): State<Arc<AppState>>,
    Path(chain_id): Path<i64>,
) -> Json<serde_json::Value> {
    match db::steps(&state.db, chain_id).await {
        Ok(rows) => Json(serde_json::json!(rows)),
        Err(e) => Json(serde_json::json!({ "error": e.to_string() })),
    }
}

pub async fn add_step(
    State(state): State<Arc<AppState>>,
    Path(chain_id): Path<i64>,
    Json(input): Json<db::AddStep>,
) -> Json<serde_json::Value> {
    match db::add_step(&state.db, chain_id, &input).await {
        Ok(id) => Json(serde_json::json!({ "ok": true, "id": id })),
        Err(e) => Json(serde_json::json!({ "error": e.to_string() })),
    }
}
