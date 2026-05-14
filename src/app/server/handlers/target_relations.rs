use crate::app::AppState;
use crate::app::database::queries::target_relations as db;
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
    Json(input): Json<db::CreateRelation>,
) -> Json<serde_json::Value> {
    match db::create(&state.db, &domain, &input).await {
        Ok(id) => Json(serde_json::json!({ "ok": true, "id": id })),
        Err(e) => Json(serde_json::json!({ "error": e.to_string() })),
    }
}
