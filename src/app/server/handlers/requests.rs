use crate::app::AppState;
use crate::app::database::queries::requests as db;
use axum::{
    Json,
    extract::{Path, State},
};
use std::sync::Arc;

pub async fn list(
    State(state): State<Arc<AppState>>,
    Path(endpoint_id): Path<i64>,
) -> Json<serde_json::Value> {
    match db::list(&state.db, endpoint_id).await {
        Ok(rows) => Json(serde_json::json!(rows)),
        Err(e) => Json(serde_json::json!({ "error": e.to_string() })),
    }
}

pub async fn create(
    State(state): State<Arc<AppState>>,
    Path(endpoint_id): Path<i64>,
    Json(input): Json<db::CreateRequest>,
) -> Json<serde_json::Value> {
    match db::create(&state.db, endpoint_id, &input).await {
        Ok(id) => Json(serde_json::json!({ "ok": true, "id": id })),
        Err(e) => Json(serde_json::json!({ "error": e.to_string() })),
    }
}

#[derive(serde::Deserialize)]
pub struct BulkSaveItem {
    pub endpoint_id: i64,
    pub raw_request: String,
    pub raw_response: Option<String>,
    pub status_code: Option<i64>,
    pub response_time_ms: Option<i64>,
    pub description: Option<String>,
    pub notes: Option<String>,
}

#[derive(serde::Deserialize)]
pub struct BulkSaveReq {
    pub items: Vec<BulkSaveItem>,
}

pub async fn bulk_save(
    State(state): State<Arc<AppState>>,
    Json(input): Json<BulkSaveReq>,
) -> Json<serde_json::Value> {
    let mut ok = 0u32;
    let mut ids = Vec::new();
    let mut errors = Vec::new();
    
    for item in input.items {
        let req_input = db::CreateRequest {
            raw_request: item.raw_request,
            raw_response: item.raw_response,
            status_code: item.status_code,
            response_time_ms: item.response_time_ms,
            description: item.description,
            notes: item.notes,
        };
        match db::create(&state.db, item.endpoint_id, &req_input).await {
            Ok(id) => {
                ok += 1;
                ids.push(id);
            }
            Err(e) => errors.push(format!("endpoint_id {}: {}", item.endpoint_id, e)),
        }
    }
    
    Json(serde_json::json!({
        "ok": ok,
        "ids": ids,
        "errors": errors
    }))
}

#[derive(serde::Deserialize)]
pub struct DiffQuery {
    pub request_id_a: i64,
    pub request_id_b: i64,
}

pub async fn diff(
    State(state): State<Arc<AppState>>,
    axum::extract::Query(params): axum::extract::Query<DiffQuery>,
) -> Json<serde_json::Value> {
    let req_a = sqlx::query_as::<_, db::RequestRow>("SELECT * FROM requests WHERE id = ?")
        .bind(params.request_id_a)
        .fetch_optional(&state.db)
        .await;

    let req_b = sqlx::query_as::<_, db::RequestRow>("SELECT * FROM requests WHERE id = ?")
        .bind(params.request_id_b)
        .fetch_optional(&state.db)
        .await;

    match (req_a, req_b) {
        (Ok(Some(a)), Ok(Some(b))) => {
            let mut diffs = Vec::new();

            if a.status_code != b.status_code {
                diffs.push(format!("STATUS: {:?} vs {:?}", a.status_code, b.status_code));
            }

            let size_a = a.raw_response.as_ref().map(|r| r.len()).unwrap_or(0);
            let size_b = b.raw_response.as_ref().map(|r| r.len()).unwrap_or(0);
            if size_a != size_b {
                diffs.push(format!("SIZE: {} bytes vs {} bytes (delta: {})", size_a, size_b, size_b as i64 - size_a as i64));
            }

            if a.response_time_ms != b.response_time_ms {
                diffs.push(format!("TIME: {:?}ms vs {:?}ms", a.response_time_ms, b.response_time_ms));
            }

            Json(serde_json::json!({
                "ok": true,
                "diffs": diffs
            }))
        }
        _ => Json(serde_json::json!({ "error": "one or both request IDs not found" })),
    }
}
