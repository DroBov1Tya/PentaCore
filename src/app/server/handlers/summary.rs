use crate::app::AppState;
use crate::app::database::queries::engagement_state;
use axum::{
    Json,
    extract::{Path, State},
};
use std::sync::Arc;

/// REST endpoint preserved for backward compatibility.
/// Internally delegates to the engagement_state recall with "progress" lens.
pub async fn get(
    State(state): State<Arc<AppState>>,
    Path(domain): Path<String>,
) -> Json<serde_json::Value> {
    match engagement_state::recall(&state.db, &domain, "progress").await {
        Ok(s) => Json(serde_json::json!(s)),
        Err(e) => Json(serde_json::json!({ "error": e.to_string() })),
    }
}
