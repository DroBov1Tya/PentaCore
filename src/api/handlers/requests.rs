use crate::app::AppState;
use crate::app::database::queries::requests as db;
use axum::{
    Json,
    extract::{Path, State},
};
use std::sync::Arc;

pub async fn list(
    State(state): State<Arc<AppState>>,
    Path(endpoint_id): Path<i64>,
) -> Json<serde_json::Value> {
    match db::list(&state.db, endpoint_id).await {
        Ok(rows) => Json(serde_json::json!(rows)),
        Err(e) => Json(serde_json::json!({ "error": e.to_string() })),
    }
}

pub async fn create(
    State(state): State<Arc<AppState>>,
    Path(endpoint_id): Path<i64>,
    Json(mut input): Json<db::CreateRequest>,
) -> Json<serde_json::Value> {
    input.endpoint_id = endpoint_id;
    match db::create(&state.db, &input).await {
        Ok(id) => Json(serde_json::json!({ "ok": true, "id": id })),
        Err(e) => Json(serde_json::json!({ "error": e.to_string() })),
    }
}
