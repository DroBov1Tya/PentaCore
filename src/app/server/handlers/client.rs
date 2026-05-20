use axum::{Json, extract::State};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use crate::app::AppState;
use crate::app::client::req::{PreRequest, make_req};
use crate::app::client::sessions::{remove_session, save_session};
use crate::app::database::queries::requests;

#[derive(Deserialize)]
pub struct SessionReq {
    pub cookies: Option<Vec<String>>,
    pub auth_token: Option<String>,
}

pub async fn set_session(Json(payload): Json<SessionReq>) -> Json<serde_json::Value> {
    let cookies = payload.cookies.unwrap_or_default();
    match save_session(cookies, payload.auth_token).await {
        Ok(_) => Json(
            serde_json::json!({ "status": "success", "message": "Session state saved successfully." }),
        ),
        Err(e) => Json(
            serde_json::json!({ "status": "error", "message": format!("Error saving session: {}", e) }),
        ),
    }
}

pub async fn revoke_session() -> Json<serde_json::Value> {
    match remove_session().await {
        Ok(_) => Json(
            serde_json::json!({ "status": "success", "message": "Session state revoked successfully." }),
        ),
        Err(e) => Json(
            serde_json::json!({ "status": "error", "message": format!("Error revoking session: {}", e) }),
        ),
    }
}

#[derive(Deserialize)]
pub struct MakeReqPayload {
    pub method: String,
    pub url: String,
    #[serde(default)]
    pub body: String,
    pub cookies: Option<Vec<String>>,
    pub proxy: Option<String>,
    pub user_agent: Option<String>,
    pub endpoint_id: Option<i64>,
}

pub async fn make_request_handler(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<MakeReqPayload>,
) -> Json<serde_json::Value> {
    let method = payload.method.to_uppercase();
    let url = payload.url.clone();
    let body = payload.body.clone();

    let prereq = PreRequest {
        cookie: payload.cookies.unwrap_or_default(),
        method: method.clone(),
        url: url.clone(),
        body: body.clone(),
        proxy: payload.proxy,
        user_agent: payload.user_agent,
        http_version: None,
        custom_headers: None,
    };

    let req_str = format!(
        "{} {}\nBody: {}",
        method,
        url,
        if body.is_empty() { "[empty]" } else { &body }
    );

    match make_req(prereq).await {
        Ok(resp) => {
            let status = resp.status().as_u16() as i64;
            let resp_text = resp.text().await.unwrap_or_default();

            if let Some(eid) = payload.endpoint_id {
                let req_input = requests::CreateRequest {
                    raw_request: req_str.clone(),
                    raw_response: Some(format!("HTTP {}\n\n{}", status, resp_text)),
                    status_code: Some(status),
                    response_time_ms: None,
                    description: Some("Auto-captured by make_request (REST)".to_string()),
                    notes: None,
                };
                let _ = requests::create(&state.db, eid, &req_input).await;
            }

            let mut result = serde_json::json!({
                "status": "success",
                "status_code": status,
                "response": resp_text
            });

            if status == 401 || status == 403 {
                result["hint"] = serde_json::Value::String("The request returned 401/403. This may indicate an expired session or missing tokens. You can use 'revoke_session' to clear invalid session state, and try to re-authenticate.".to_string());
            }
            Json(result)
        }
        Err(e) => Json(
            serde_json::json!({ "status": "error", "message": format!("Request error: {}", e) }),
        ),
    }
}

#[derive(Deserialize)]
pub struct RaceReqPayload {
    pub method: String,
    pub url: String,
    #[serde(default)]
    pub body: String,
    #[serde(default)]
    pub count: Option<i64>,
    #[serde(default)]
    pub threads: Option<i64>,
    pub cookies: Option<Vec<String>>,
    pub proxy: Option<String>,
    pub user_agent: Option<String>,
}

pub async fn make_race_requests_handler(
    Json(payload): Json<RaceReqPayload>,
) -> Json<serde_json::Value> {
    let count = payload.count.unwrap_or(5).clamp(1, 100) as usize;
    let threads = payload.threads.unwrap_or(5).clamp(1, 20) as usize;

    let semaphore = std::sync::Arc::new(tokio::sync::Semaphore::new(threads));
    let mut handles = Vec::new();

    let cookies = payload.cookies.unwrap_or_default();

    for _ in 0..count {
        let prereq = PreRequest {
            cookie: cookies.clone(),
            method: payload.method.clone(),
            url: payload.url.clone(),
            body: payload.body.clone(),
            proxy: payload.proxy.clone(),
            user_agent: payload.user_agent.clone(),
            http_version: None,
            custom_headers: None,
        };
        let sem = semaphore.clone();
        handles.push(tokio::spawn(async move {
            let _permit = sem.acquire().await;
            make_req(prereq).await
        }));
    }

    let mut summary = Vec::new();
    for (i, handle) in handles.into_iter().enumerate() {
        let result_str = match handle.await {
            Ok(Ok(resp)) => format!("Status {}", resp.status().as_u16()),
            Ok(Err(e)) => format!("Error {}", e),
            Err(e) => format!("Task Panic {}", e),
        };
        summary.push(serde_json::json!({ "request_id": i + 1, "result": result_str }));
    }

    Json(serde_json::json!({
        "status": "success",
        "total_requests": count,
        "concurrent_threads": threads,
        "results": summary
    }))
}
