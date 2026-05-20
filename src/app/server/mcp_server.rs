use anyhow::Result;
use serde_json::{Value, json};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

use crate::app::AppState;
mod handlers;
mod messages;
pub mod tool_registry;

static USE_CONTENT_LENGTH: AtomicBool = AtomicBool::new(false);

pub async fn start(state: Arc<AppState>) -> Result<()> {
    let mut stdin = tokio::io::stdin();
    let mut shutdown_rx = state.shutdown.subscribe();

    tracing::info!("🤖 MCP stdio transport started");

    let mut buffer = Vec::new();
    let mut chunk = [0u8; 8192];

    loop {
        tokio::select! {
            _ = shutdown_rx.recv() => break,
            read_result = stdin.read(&mut chunk) => {
                match read_result {
                    Ok(0) => break, // EOF
                    Ok(n) => {
                        buffer.extend_from_slice(&chunk[..n]);
                        process_buffer(&mut buffer, &state).await;
                    }
                    Err(e) => {
                        tracing::error!("stdin read error: {}", e);
                        break;
                    }
                }
            }
        }
    }
    Ok(())
}

async fn process_buffer(buffer: &mut Vec<u8>, state: &Arc<AppState>) {
    loop {
        if buffer.starts_with(b"Content-Length: ") {
            USE_CONTENT_LENGTH.store(true, Ordering::Relaxed);
            if let Some(pos) = buffer.windows(4).position(|w| w == b"\r\n\r\n") {
                let header = std::str::from_utf8(&buffer[16..pos]).unwrap_or("").trim();
                if let Ok(content_length) = header.parse::<usize>() {
                    let total_length = pos + 4 + content_length;
                    if buffer.len() >= total_length {
                        let json_bytes = &buffer[pos + 4..total_length];
                        if let Ok(req) = serde_json::from_slice::<Value>(json_bytes) {
                            let resp = handle_request(&req, state).await;
                            if !resp.is_null() {
                                send_response(&resp).await;
                            }
                        }
                        buffer.drain(..total_length);
                        continue;
                    }
                }
            }
            break;
        } else {
            if let Some(pos) = buffer.iter().position(|&b| b == b'\n' || b == b'\r') {
                if pos == 0 {
                    buffer.drain(..1);
                    continue;
                }
                let json_bytes = &buffer[..pos];
                if let Ok(req) = serde_json::from_slice::<Value>(json_bytes) {
                    let resp = handle_request(&req, state).await;
                    if !resp.is_null() {
                        send_response(&resp).await;
                    }
                }
                buffer.drain(..pos + 1);
                continue;
            }
            break;
        }
    }
}

async fn send_response(resp: &Value) {
    let json_str = resp.to_string();
    let mut stdout = tokio::io::stdout();
    if USE_CONTENT_LENGTH.load(Ordering::Relaxed) {
        let header = format!("Content-Length: {}\r\n\r\n{}", json_str.len(), json_str);
        let _ = stdout.write_all(header.as_bytes()).await;
    } else {
        let msg = format!("{}\n", json_str);
        let _ = stdout.write_all(msg.as_bytes()).await;
    }
    let _ = stdout.flush().await;
}

pub async fn handle_request(req: &Value, state: &Arc<AppState>) -> Value {
    let id = &req["id"];
    let method = req["method"].as_str().unwrap_or("");

    let response = match method {
        "initialize" => {
            let requested_version = req["params"]["protocolVersion"]
                .as_str()
                .unwrap_or("2024-11-05");
            messages::initialize_msg(id, requested_version)
        }
        "notifications/initialized" => return Value::Null,
        "resources/list" => messages::resources_list_msg(id),
        "resources/read" => {
            let uri = req["params"]["uri"].as_str().unwrap_or("");
            if uri == "pentest://instructions" {
                messages::resources_read_msg(id, &state.cfg.server_path)
            } else {
                json!({ "jsonrpc": "2.0", "id": id, "error": { "code": -32602, "message": "Resource not found" } })
            }
        }
        "tools/list" => messages::tools_list_msg(id),
        "ping" | "notifications/ping" => json!({ "jsonrpc": "2.0", "id": id, "result": {} }),
        "tools/call" => {
            let tool_name = req["params"]["name"].as_str().unwrap_or("");
            let args = &req["params"]["arguments"];

            match execute_tool(tool_name, args, state).await {
                Ok(result_text) => {
                    json!({
                        "jsonrpc": "2.0",
                        "id": id,
                        "result": { "content": [{ "type": "text", "text": result_text }] }
                    })
                }
                Err(e) => {
                    json!({
                        "jsonrpc": "2.0",
                        "id": id,
                        "result": {
                            "content": [{ "type": "text", "text": format!("Error: {}", e) }],
                            "isError": true
                        }
                    })
                }
            }
        }
        _ => {
            json!({ "jsonrpc": "2.0", "id": id, "error": { "code": -32601, "message": "Method not found" } })
        }
    };

    response
}

async fn execute_tool(name: &str, args: &Value, state: &Arc<AppState>) -> Result<String> {
    state
        .registry
        .dispatch(name, args.clone(), Arc::clone(state))
        .await
}
