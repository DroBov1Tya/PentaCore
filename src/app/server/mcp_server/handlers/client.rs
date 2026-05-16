use crate::app::AppState;
use crate::app::database::queries::requests;
use serde_json::Value;
use std::sync::Arc;

/// Updates the global HTTP session state.
/// This includes setting persistent cookies and/or an authorization token (e.g., Bearer token)
/// that will be injected into all subsequent requests made via `make_request` or `make_race_requests`.
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

/// Clears the current global HTTP session state.
/// This completely drops all active cookies and authorization tokens,
/// ensuring future requests are completely unauthenticated.
pub async fn handle_revoke_session(_args: &Value, _state: &Arc<AppState>) -> String {
    match crate::app::client::sessions::remove_session().await {
        Ok(_) => "Session state revoked successfully.".to_string(),
        Err(e) => format!("Error revoking session: {}", e),
    }
}

/// Constructs and executes a single HTTP request using the provided arguments and global session.
/// Automatically handles proxy routing, custom user agents, and response parsing.
/// If `endpoint_id` is provided, the request and its response are automatically logged to the DB.
/// To protect agent context, the response body is strictly truncated at 500KB.
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
            let max_size: u64 = 500 * 1024;
            let content_len = resp.content_length().unwrap_or(0);

            let mut resp_text = String::new();
            if content_len > max_size {
                resp_text = format!(
                    "[TRUNCATED: Content-Length reported as {} bytes, exceeding 500KB limit]",
                    content_len
                );
            } else {
                use futures::StreamExt;
                let mut stream = resp.bytes_stream();
                let mut total_read = 0;
                while let Some(chunk) = stream.next().await {
                    if let Ok(bytes) = chunk {
                        total_read += bytes.len() as u64;
                        if total_read > max_size {
                            resp_text
                                .push_str("\n\n[...TRUNCATED: Response exceeded 500KB limit...]");
                            break;
                        }
                        resp_text.push_str(&String::from_utf8_lossy(&bytes));
                    }
                }
            }

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

            let response_obj = serde_json::json!({
                "status": status,
                "hint": if hint.is_empty() { serde_json::Value::Null } else { serde_json::Value::String(hint.trim().to_string()) },
                "body": resp_text
            });
            serde_json::to_string_pretty(&response_obj).unwrap_or_default()
        }
        Err(e) => format!("Request error: {}", e),
    }
}

/// Executes a high-concurrency race condition (TOCTOU) test.
/// Spawns multiple identical HTTP requests simultaneously using a semaphore-controlled thread pool.
/// Returns a summary of the status codes received across all concurrent requests.
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

/// Automates IDOR and authorization testing by replaying a previously saved request with a new identity.
/// It fetches the original request by ID, strips the old cookies, injects the provided new cookies,
/// and executes the modified request. The new request/response is automatically saved as evidence.
pub async fn handle_replay_as(args: &Value, state: &Arc<AppState>) -> String {
    let id = match args["request_id"].as_i64() {
        Some(id) => id,
        None => return "Error: request_id is required".to_string(),
    };

    let req = match requests::get(&state.db, id).await {
        Ok(Some(r)) => r,
        _ => return format!("Error: Request {} not found", id),
    };

    let raw = req.raw_request;
    let lines: Vec<&str> = raw.lines().collect();
    if lines.is_empty() {
        return "Error: raw_request is empty".to_string();
    }

    let first_line = lines[0];
    let parts: Vec<&str> = first_line.splitn(2, ' ').collect();
    if parts.len() != 2 {
        return "Error: Could not parse METHOD and URL from raw_request".to_string();
    }
    let method = parts[0].to_string();
    let url = parts[1].to_string();

    let mut body = String::new();
    let mut in_body = false;
    for line in lines.iter().skip(1) {
        if line.starts_with("Body: ") {
            in_body = true;
            let b = line.strip_prefix("Body: ").unwrap();
            if b != "[empty]" {
                body.push_str(b);
            }
            continue;
        }
        if in_body {
            body.push('\n');
            body.push_str(line);
        }
    }

    let mut cookies = args["cookies"]
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();

    let prereq = crate::app::client::req::PreRequest {
        cookie: cookies,
        method: method.clone(),
        url: url.clone(),
        body: body.clone(),
        proxy: None,
        user_agent: None,
    };

    match crate::app::client::req::make_req(prereq).await {
        Ok(resp) => {
            let status = resp.status().as_u16() as i64;
            let resp_text = resp.text().await.unwrap_or_default();

            let req_str = format!(
                "{} {}\nBody: {}",
                method,
                url,
                if body.is_empty() { "[empty]" } else { &body }
            );

            let req_input = requests::CreateRequest {
                raw_request: req_str.clone(),
                raw_response: Some(format!("HTTP {}\n\n{}", status, resp_text)),
                status_code: Some(status),
                response_time_ms: None,
                description: Some(format!("Replay of request {}", id)),
                notes: None,
            };

            let new_id_str = match requests::create(&state.db, req.endpoint_id, &req_input).await {
                Ok(new_id) => format!("(Saved as Request ID {})", new_id),
                Err(_) => "(Failed to save to DB)".to_string(),
            };

            let response_obj = serde_json::json!({
                "status": status,
                "body": resp_text,
                "note": format!("Replay successful. {}", new_id_str)
            });
            serde_json::to_string_pretty(&response_obj).unwrap_or_default()
        }
        Err(e) => format!("Request error: {}", e),
    }
}
