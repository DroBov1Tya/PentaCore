use crate::app::AppState;
use crate::app::database::queries::endpoints as db;
use axum::{
    Json,
    extract::{Path, Query, State},
};
use serde::Deserialize;
use std::sync::Arc;

#[derive(Deserialize)]
pub struct EndpointFilter {
    pub status: Option<i64>,
}

pub async fn list(
    State(state): State<Arc<AppState>>,
    Path(domain): Path<String>,
    Query(filter): Query<EndpointFilter>,
) -> Json<serde_json::Value> {
    match db::list(&state.db, &domain, filter.status).await {
        Ok(rows) => Json(serde_json::json!(rows)),
        Err(e) => Json(serde_json::json!({ "error": e.to_string() })),
    }
}

pub async fn create(
    State(state): State<Arc<AppState>>,
    Path(domain): Path<String>,
    Json(input): Json<db::CreateEndpoint>,
) -> Json<serde_json::Value> {
    match db::create(&state.db, &domain, &input).await {
        Ok(id) => Json(serde_json::json!({ "ok": true, "id": id })),
        Err(e) => Json(serde_json::json!({ "error": e.to_string() })),
    }
}
