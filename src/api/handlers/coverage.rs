use crate::app::AppState;
use crate::app::database::queries::coverage as db;
use axum::{
    Json,
    extract::{Path, Query, State},
};
use serde::Deserialize;
use std::sync::Arc;

#[derive(Deserialize)]
pub struct CoverageFilter {
    pub status: Option<String>,
}

pub async fn list(
    State(state): State<Arc<AppState>>,
    Path(endpoint_id): Path<i64>,
    Query(filter): Query<CoverageFilter>,
) -> Json<serde_json::Value> {
    match db::list(&state.db, endpoint_id, filter.status.as_deref()).await {
        Ok(rows) => Json(serde_json::json!(rows)),
        Err(e) => Json(serde_json::json!({ "error": e.to_string() })),
    }
}

pub async fn upsert(
    State(state): State<Arc<AppState>>,
    Path(endpoint_id): Path<i64>,
    Json(mut input): Json<db::UpsertCoverage>,
) -> Json<serde_json::Value> {
    input.endpoint_id = endpoint_id;
    match db::upsert(&state.db, &input).await {
        Ok(_) => Json(serde_json::json!({ "ok": true })),
        Err(e) => Json(serde_json::json!({ "error": e.to_string() })),
    }
}
