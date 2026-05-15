use crate::app::AppState;
use crate::app::database::queries::requests;
use serde_json::Value;
use std::sync::Arc;

pub async fn handle_set_session(args: &Value, _state: &Arc<AppState>) -> String {
    let cookies = args["cookies"]
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let auth_token = args["auth_token"].as_str().map(String::from);
    match crate::app::client::sessions::save_session(cookies, auth_token).await {
        Ok(_) => "Session state saved successfully.".to_string(),
        Err(e) => format!("Error saving session: {}", e),
    }
}

pub async fn handle_revoke_session(_args: &Value, _state: &Arc<AppState>) -> String {
    match crate::app::client::sessions::remove_session().await {
        Ok(_) => "Session state revoked successfully.".to_string(),
        Err(e) => format!("Error revoking session: {}", e),
    }
}

pub async fn handle_make_request(args: &Value, state: &Arc<AppState>) -> String {
    let method = args["method"].as_str().unwrap_or("GET").to_string();
    let url = args["url"].as_str().unwrap_or("").to_string();
    let body = args["body"].as_str().unwrap_or("").to_string();
    let proxy = args["proxy"].as_str().map(String::from);
    let user_agent = args["user_agent"].as_str().map(String::from);
    let cookies = args["cookies"]
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let endpoint_id = args["endpoint_id"].as_i64();

    let prereq = crate::app::client::req::PreRequest {
        cookie: cookies,
        method: method.clone(),
        url: url.clone(),
        body: body.clone(),
        proxy,
        user_agent,
    };

    let req_str = format!(
        "{} {}\nBody: {}",
        method,
        url,
        if body.is_empty() { "[empty]" } else { &body }
    );
    match crate::app::client::req::make_req(prereq).await {
        Ok(resp) => {
            let status = resp.status().as_u16() as i64;
            let resp_text = resp.text().await.unwrap_or_default();

            if let Some(eid) = endpoint_id {
                let req_input = requests::CreateRequest {
                    raw_request: req_str.clone(),
                    raw_response: Some(format!("HTTP {}\n\n{}", status, resp_text)),
                    status_code: Some(status),
                    response_time_ms: None,
                    description: Some("Auto-captured by make_request".to_string()),
                    notes: None,
                };
                let _ = requests::create(&state.db, eid, &req_input).await;
            }

            let mut hint = if status == 401 || status == 403 {
                "\n\n[HINT]: The request returned 401/403. This may indicate an expired session or missing tokens. You can use 'revoke_session' to clear invalid session state, and try to re-authenticate.".to_string()
            } else {
                "".to_string()
            };

            if let Some(recon_hint) = crate::app::client::recon::analyze_response(&resp_text) {
                hint.push_str(&recon_hint);
            }

            format!("Status: {}{}\n\n{}", status, hint, resp_text)
        }
        Err(e) => format!("Request error: {}", e),
    }
}

pub async fn handle_make_race_requests(args: &Value, _state: &Arc<AppState>) -> String {
    let method = args["method"].as_str().unwrap_or("GET").to_string();
    let url = args["url"].as_str().unwrap_or("").to_string();
    let body = args["body"].as_str().unwrap_or("").to_string();
    let count = args["count"].as_i64().unwrap_or(5).clamp(1, 100) as usize;
    let threads = args["threads"].as_i64().unwrap_or(5).clamp(1, 20) as usize;
    let cookies = args["cookies"]
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let user_agent = args["user_agent"].as_str().map(String::from);
    let proxy = args["proxy"].as_str().map(String::from);

    let semaphore = std::sync::Arc::new(tokio::sync::Semaphore::new(threads));
    let mut handles = Vec::new();

    for _ in 0..count {
        let prereq = crate::app::client::req::PreRequest {
            cookie: cookies.clone(),
            method: method.clone(),
            url: url.clone(),
            body: body.clone(),
            proxy: proxy.clone(),
            user_agent: user_agent.clone(),
        };
        let sem = semaphore.clone();
        handles.push(tokio::spawn(async move {
            let _permit = sem.acquire().await;
            crate::app::client::req::make_req(prereq).await
        }));
    }

    let mut summary = String::new();
    for (i, handle) in handles.into_iter().enumerate() {
        match handle.await {
            Ok(Ok(resp)) => {
                let status = resp.status().as_u16();
                summary.push_str(&format!("Request {}: Status {}\n", i + 1, status));
            }
            Ok(Err(e)) => summary.push_str(&format!("Request {}: Error {}\n", i + 1, e)),
            Err(e) => summary.push_str(&format!("Request {}: Task Panic {}\n", i + 1, e)),
        }
    }
    format!(
        "Race Condition Test Results ({} total requests, {} concurrent threads):\n{}",
        count, threads, summary
    )
}
