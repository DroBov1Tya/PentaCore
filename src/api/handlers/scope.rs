use crate::app::AppState;
use crate::app::database::queries::scope as db;
use axum::{
    Json,
    extract::{Path, State},
};
use std::sync::Arc;

pub async fn get(
    State(state): State<Arc<AppState>>,
    Path(domain): Path<String>,
) -> Json<serde_json::Value> {
    match db::get(&state.db, &domain).await {
        Ok(Some(scope)) => Json(serde_json::json!(scope)),
        Ok(None) => Json(serde_json::json!({ "error": "no scope defined" })),
        Err(e) => Json(serde_json::json!({ "error": e.to_string() })),
    }
}

pub async fn upsert(
    State(state): State<Arc<AppState>>,
    Path(domain): Path<String>,
    Json(input): Json<db::CreateScope>,
) -> Json<serde_json::Value> {
    match db::upsert(&state.db, &domain, &input).await {
        Ok(id) => Json(serde_json::json!({ "ok": true, "id": id })),
        Err(e) => Json(serde_json::json!({ "error": e.to_string() })),
    }
}
