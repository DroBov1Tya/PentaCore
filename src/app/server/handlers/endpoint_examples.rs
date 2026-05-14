use crate::app::database::queries::endpoint_examples;
use crate::app::AppState;
use axum::{
    extract::{Path, State},
    Json,
};
use std::sync::Arc;

pub async fn get(
    State(state): State<Arc<AppState>>,
    Path(endpoint_id): Path<i64>,
) -> Result<Json<endpoint_examples::EndpointExampleRow>, String> {
    match endpoint_examples::get(&state.db, endpoint_id).await {
        Ok(Some(s)) => Ok(Json(s)),
        Ok(None) => Err("Example not found".to_string()),
        Err(e) => Err(e.to_string()),
    }
}

pub async fn upsert(
    State(state): State<Arc<AppState>>,
    Path(endpoint_id): Path<i64>,
    Json(payload): Json<endpoint_examples::SaveExample>,
) -> Result<Json<i64>, String> {
    match endpoint_examples::upsert(&state.db, endpoint_id, &payload).await {
        Ok(id) => Ok(Json(id)),
        Err(e) => Err(e.to_string()),
    }
}
