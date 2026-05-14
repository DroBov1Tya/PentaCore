use crate::app::AppState;
use crate::app::database::queries::findings as db;
use axum::{
    Json,
    extract::{Path, Query, State},
};
use serde::Deserialize;
use std::sync::Arc;

#[derive(Deserialize)]
pub struct FindingFilter {
    pub severity: Option<String>,
    pub status: Option<String>,
}

pub async fn list(
    State(state): State<Arc<AppState>>,
    Path(domain): Path<String>,
    Query(filter): Query<FindingFilter>,
) -> Json<serde_json::Value> {
    match db::list(
        &state.db,
        &domain,
        filter.severity.as_deref(),
        filter.status.as_deref(),
    )
    .await
    {
        Ok(rows) => Json(serde_json::json!(rows)),
        Err(e) => Json(serde_json::json!({ "error": e.to_string() })),
    }
}

pub async fn create(
    State(state): State<Arc<AppState>>,
    Path(domain): Path<String>,
    Json(input): Json<db::CreateFinding>,
) -> Json<serde_json::Value> {
    match db::create(&state.db, &domain, &input).await {
        Ok(id) => Json(serde_json::json!({ "ok": true, "id": id })),
        Err(e) => Json(serde_json::json!({ "error": e.to_string() })),
    }
}
