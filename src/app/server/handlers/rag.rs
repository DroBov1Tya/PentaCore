use crate::app::AppState;
use crate::app::rag::types::SearchResult;
use axum::{
    Json,
    extract::{Path, Query, State},
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

#[derive(Deserialize)]
pub struct MemorizeReq {
    pub category: Option<String>,
    pub title: String,
    pub content: String,
    pub tags: Option<Vec<String>>,
}

#[derive(Serialize)]
pub struct MemorizeResp {
    pub id: String,
}

pub async fn memorize(
    State(state): State<Arc<AppState>>,
    Path(domain): Path<String>,
    Json(payload): Json<MemorizeReq>,
) -> Result<Json<MemorizeResp>, String> {
    let mut store = state.memory_store.lock().await;
    let category = payload.category.as_deref().unwrap_or("note");
    let tags = payload.tags.unwrap_or_default();

    match store
        .memorize(&domain, category, &payload.title, &payload.content, &tags)
        .await
    {
        Ok(id) => Ok(Json(MemorizeResp { id })),
        Err(e) => Err(e.to_string()),
    }
}

#[derive(Deserialize)]
pub struct SearchQuery {
    pub query: String,
    pub domain: Option<String>,
    pub limit: Option<usize>,
}

pub async fn search(
    State(state): State<Arc<AppState>>,
    Query(params): Query<SearchQuery>,
) -> Result<Json<SearchResult>, String> {
    let mut store = state.memory_store.lock().await;
    let limit = params.limit.unwrap_or(5);

    match store
        .search(&params.query, params.domain.as_deref(), limit)
        .await
    {
        Ok(res) => Ok(Json(res)),
        Err(e) => Err(e.to_string()),
    }
}

#[derive(Deserialize)]
pub struct ListQuery {
    pub domain: Option<String>,
    pub limit: Option<usize>,
}

pub async fn list_memories(
    State(state): State<Arc<AppState>>,
    Query(params): Query<ListQuery>,
) -> Result<Json<Vec<crate::app::rag::types::MemoryNote>>, String> {
    let store = state.memory_store.lock().await;
    let limit = params.limit.unwrap_or(10);

    match store.list_memories(params.domain.as_deref(), limit).await {
        Ok(res) => Ok(Json(res)),
        Err(e) => Err(e.to_string()),
    }
}

#[derive(Serialize)]
pub struct ForgetResp {
    pub success: bool,
}

pub async fn forget(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> Result<Json<ForgetResp>, String> {
    let store = state.memory_store.lock().await;
    match store.forget(&id).await {
        Ok(_) => Ok(Json(ForgetResp { success: true })),
        Err(e) => Err(e.to_string()),
    }
}

pub async fn get_memory(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> Result<Json<crate::app::rag::types::MemoryNote>, String> {
    let store = state.memory_store.lock().await;
    match store.get_memory(&id).await {
        Ok(Some(mem)) => Ok(Json(mem)),
        Ok(None) => Err(format!("Memory with id {} not found", id)),
        Err(e) => Err(e.to_string()),
    }
}

#[derive(Deserialize)]
pub struct UpdateMemoryReq {
    pub category: Option<String>,
    pub title: Option<String>,
    pub content: Option<String>,
    pub tags: Option<Vec<String>>,
}

pub async fn update_memory(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    Json(payload): Json<UpdateMemoryReq>,
) -> Result<Json<ForgetResp>, String> {
    let mut store = state.memory_store.lock().await;
    match store
        .update_memory(
            &id,
            payload.category.as_deref(),
            payload.title.as_deref(),
            payload.content.as_deref(),
            payload.tags.as_deref(),
        )
        .await
    {
        Ok(_) => Ok(Json(ForgetResp { success: true })),
        Err(e) => Err(e.to_string()),
    }
}
