use anyhow::Result;
use serde_json::{Value, json};
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, BufReader};

use crate::app::AppState;
mod handlers;
mod messages;

pub async fn start(state: Arc<AppState>) -> Result<()> {
    let stdin = tokio::io::stdin();
    let mut reader = BufReader::new(stdin).lines();
    let mut shutdown_rx = state.shutdown.subscribe();

    tracing::info!("🤖 MCP stdio transport started");

    loop {
        tokio::select! {
            _ = shutdown_rx.recv() => break,
            line = reader.next_line() => {
                match line {
                    Ok(Some(line)) => {
                        if let Ok(req) = serde_json::from_str::<Value>(&line) {
                            let resp = handle_request(&req, &state).await;
                            if !resp.is_null() {
                                println!("{}", resp.to_string());
                            }
                        }
                    }
                    Ok(None) => break,
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

pub async fn handle_request(req: &Value, state: &Arc<AppState>) -> Value {
    let id = &req["id"];
    let method = req["method"].as_str().unwrap_or("");

    let response = match method {
        "initialize" => messages::initialize_msg(id),
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

            let result_text = execute_tool(tool_name, args, state).await;

            json!({
                "jsonrpc": "2.0",
                "id": id,
                "result": { "content": [{ "type": "text", "text": result_text }] }
            })
        }
        _ => {
            json!({ "jsonrpc": "2.0", "id": id, "error": { "code": -32601, "message": "Method not found" } })
        }
    };

    response
}

async fn execute_tool(name: &str, args: &Value, state: &Arc<AppState>) -> String {
    match name {
        "get_summary" => handlers::scope::handle_get_summary(args, state).await,
        "get_scope" => handlers::scope::handle_get_scope(args, state).await,
        "save_scope" => handlers::scope::handle_save_scope(args, state).await,
        "get_relations" => handlers::recon::handle_get_relations(args, state).await,
        "save_relation" => handlers::recon::handle_save_relation(args, state).await,
        "get_endpoints" => handlers::recon::handle_get_endpoints(args, state).await,
        "save_endpoint" => handlers::recon::handle_save_endpoint(args, state).await,
        "enumerate_subdomains" => handlers::recon::handle_enumerate_subdomains(args, state).await,
        "resolve_dns" => handlers::recon::handle_resolve_dns(args, state).await,
        "memorize_concept" => handlers::rag::handle_memorize_concept(args, state).await,
        "search_knowledge" => handlers::rag::handle_search_knowledge(args, state).await,
        "list_memories" => handlers::rag::handle_list_memories(args, state).await,
        "forget_memory" => handlers::rag::handle_forget_memory(args, state).await,
        "get_memory" => handlers::rag::handle_get_memory(args, state).await,
        "update_memory" => handlers::rag::handle_update_memory(args, state).await,
        "get_findings" => handlers::vulns::handle_get_findings(args, state).await,
        "save_finding" => handlers::vulns::handle_save_finding(args, state).await,
        "get_credentials" => handlers::vulns::handle_get_credentials(args, state).await,
        "save_credential" => handlers::vulns::handle_save_credential(args, state).await,
        "get_chains" => handlers::vulns::handle_get_chains(args, state).await,
        "save_chain" => handlers::vulns::handle_save_chain(args, state).await,
        "get_chain_steps" => handlers::vulns::handle_get_chain_steps(args, state).await,
        "add_chain_step" => handlers::vulns::handle_add_chain_step(args, state).await,
        "set_session" => handlers::client::handle_set_session(args, state).await,
        "revoke_session" => handlers::client::handle_revoke_session(args, state).await,
        "make_request" => handlers::client::handle_make_request(args, state).await,
        "make_race_requests" => handlers::client::handle_make_race_requests(args, state).await,
        "get_coverage" => handlers::coverage_requests::handle_get_coverage(args, state).await,
        "upsert_coverage" => handlers::coverage_requests::handle_upsert_coverage(args, state).await,
        "bulk_upsert_coverage" => handlers::coverage_requests::handle_bulk_upsert_coverage(args, state).await,
        "get_requests" => handlers::coverage_requests::handle_get_requests(args, state).await,
        "save_request" => handlers::coverage_requests::handle_save_request(args, state).await,
        "bulk_save_requests" => handlers::coverage_requests::handle_bulk_save_requests(args, state).await,
        "diff_requests" => handlers::coverage_requests::handle_diff_requests(args, state).await,
        "claim_test_object" => handlers::test_objects::handle_claim_test_object(args, state).await,
        "rollback_test_object" => handlers::test_objects::handle_rollback_test_object(args, state).await,
        "get_test_objects" => handlers::test_objects::handle_get_test_objects(args, state).await,
        "save_endpoint_example" => handlers::parsers::handle_save_endpoint_example(args, state).await,
        "get_endpoint_example" => handlers::parsers::handle_get_endpoint_example(args, state).await,
        "parse_api_spec" => handlers::parsers::handle_parse_api_spec(args, state).await,
        "parse_graphql_spec" => handlers::parsers::handle_parse_graphql_spec(args, state).await,
        _ => format!("Unknown tool: {}", name),
    }
}
