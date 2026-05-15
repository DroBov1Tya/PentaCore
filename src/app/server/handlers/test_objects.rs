use crate::app::AppState;
use crate::app::database::queries::test_objects;
use axum::{
    Json,
    extract::{Path, Query, State},
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

#[derive(Deserialize)]
pub struct ListQuery {
    pub status: Option<String>,
}

pub async fn list(
    State(state): State<Arc<AppState>>,
    Path(domain): Path<String>,
    Query(params): Query<ListQuery>,
) -> Result<Json<Vec<test_objects::TestObjectRow>>, String> {
    match test_objects::list(&state.db, &domain, params.status.as_deref()).await {
        Ok(s) => Ok(Json(s)),
        Err(e) => Err(e.to_string()),
    }
}

pub async fn claim(
    State(state): State<Arc<AppState>>,
    Path(domain): Path<String>,
    Json(payload): Json<test_objects::ClaimTestObject>,
) -> Result<Json<i64>, String> {
    match test_objects::claim(&state.db, &domain, &payload).await {
        Ok(id) => Ok(Json(id)),
        Err(e) => Err(e.to_string()),
    }
}

pub async fn rollback(
    State(state): State<Arc<AppState>>,
    Path(id): Path<i64>,
) -> Result<Json<test_objects::TestObjectRow>, String> {
    match test_objects::rollback(&state.db, id).await {
        Ok(Some(obj)) => Ok(Json(obj)),
        Ok(None) => Err("Object not found".to_string()),
        Err(e) => Err(e.to_string()),
    }
}
