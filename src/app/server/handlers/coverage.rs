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
    Json(input): Json<db::UpsertCoverage>,
) -> Json<serde_json::Value> {
    match db::upsert(&state.db, endpoint_id, &input).await {
        Ok(_) => Json(serde_json::json!({ "ok": true })),
        Err(e) => Json(serde_json::json!({ "error": e.to_string() })),
    }
}

#[derive(Deserialize)]
pub struct BulkUpsertItem {
    pub endpoint_id: i64,
    pub vector: String,
    pub status: String,
    pub description: Option<String>,
    pub notes: Option<String>,
}

#[derive(Deserialize)]
pub struct BulkUpsertReq {
    pub items: Vec<BulkUpsertItem>,
}

pub async fn bulk_upsert(
    State(state): State<Arc<AppState>>,
    Json(input): Json<BulkUpsertReq>,
) -> Json<serde_json::Value> {
    let mut ok = 0u32;
    let mut errors = Vec::new();
    
    for item in input.items {
        let upsert_input = db::UpsertCoverage {
            vector: item.vector,
            status: item.status,
            description: item.description,
            notes: item.notes,
        };
        match db::upsert(&state.db, item.endpoint_id, &upsert_input).await {
            Ok(_) => ok += 1,
            Err(e) => errors.push(format!("endpoint_id {}: {}", item.endpoint_id, e)),
        }
    }
    
    Json(serde_json::json!({
        "ok": ok,
        "errors": errors
    }))
}
