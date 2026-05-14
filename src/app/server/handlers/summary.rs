use crate::app::AppState;
use crate::app::database::queries::summary as db;
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
        Ok(Some(s)) => Json(serde_json::json!(s)),
        Ok(None) => Json(serde_json::json!({ "error": "domain not found" })),
        Err(e) => Json(serde_json::json!({ "error": e.to_string() })),
    }
}
