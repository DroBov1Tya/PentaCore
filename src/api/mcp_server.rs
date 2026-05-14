use anyhow::Result;
use serde_json::{Value, json};
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, BufReader};

use crate::app::AppState;
use crate::app::database::queries::{
    attack_chains, coverage, credentials, endpoints, findings, requests, scope, summary,
    target_relations,
};

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
        "initialize" => json!({
            "jsonrpc": "2.0",
            "id": id,
            "result": {
                "protocolVersion": "2024-11-05",
                "capabilities": {
                    "tools": {},
                    "resources": {}
                },
                "serverInfo": {
                    "name": "hackstorage-mcp-stdio",
                    "version": crate::constants::VERSION
                }
            }
        }),
        "notifications/initialized" => return Value::Null,
        "resources/list" => json!({
            "jsonrpc": "2.0",
            "id": id,
            "result": {
                "resources": [
                    {
                        "uri": "pentest://instructions",
                        "name": "Pentest Context MCP Instructions",
                        "description": "Rules and manual for using HackStorage. Read this to understand how to store context properly."
                    }
                ]
            }
        }),
        "resources/read" => {
            let uri = req["params"]["uri"].as_str().unwrap_or("");
            if uri == "pentest://instructions" {
                json!({
                    "jsonrpc": "2.0",
                    "id": id,
                    "result": {
                        "contents": [{
                            "uri": "pentest://instructions",
                            "mimeType": "text/markdown",
                            "text": format!("## HackStorage MCP
Persistent context store for pentest sessions. Saves tokens by giving structured, queryable memory across sessions.
**NOTE TO AI:** You can use this MCP server OR you can make standard HTTP REST requests to localhost:{} if you find it more convenient. Both methods work and modify the same database.

### Rules
- ALWAYS start session with `get_scope`
- A finding is confirmed only with a reproducible PoC — use status=potential until then
- Save raw request and response for every finding — this is your evidence base
- No findings means incomplete coverage, not a clean target
- Check coverage before closing a phase", state.cfg.server_path.split(':').last().unwrap_or("8082")
                            )}]
                    }
                })
            } else {
                json!({ "jsonrpc": "2.0", "id": id, "error": { "code": -32602, "message": "Resource not found" } })
            }
        }
        "tools/list" => json!({
            "jsonrpc": "2.0",
            "id": id,
            "result": {
                "tools": [
                    {
                        "name": "get_summary",
                        "description": "Full target picture in one request. Restores session context.",
                        "inputSchema": { "type": "object", "properties": { "domain": { "type": "string" } }, "required": ["domain"] }
                    },
                    {
                        "name": "get_scope",
                        "description": "Get engagement rules and scope for a target.",
                        "inputSchema": { "type": "object", "properties": { "domain": { "type": "string" } }, "required": ["domain"] }
                    },
                    {
                        "name": "save_scope",
                        "description": "Save or update engagement rules.",
                        "inputSchema": {
                            "type": "object",
                            "properties": {
                                "domain": { "type": "string" }, "objective": { "type": "string" },
                                "in_scope": { "type": "string" }, "out_of_scope": { "type": "string" }, "rules": { "type": "string" }
                            },
                            "required": ["domain", "objective", "in_scope"]
                        }
                    },
                    {
                        "name": "get_relations",
                        "description": "Get domain relationships (subdomains, pivots).",
                        "inputSchema": { "type": "object", "properties": { "domain": { "type": "string" } }, "required": ["domain"] }
                    },
                    {
                        "name": "save_relation",
                        "description": "Save domain relationship.",
                        "inputSchema": {
                            "type": "object",
                            "properties": {
                                "from_domain": { "type": "string" }, "to_domain": { "type": "string" },
                                "rel_type": { "type": "string", "enum": ["subdomain", "cdn", "shared_infra", "pivot", "related"] },
                                "description": { "type": "string" }
                            },
                            "required": ["from_domain", "to_domain", "rel_type"]
                        }
                    },
                    {
                        "name": "get_endpoints",
                        "description": "List discovered endpoints.",
                        "inputSchema": {
                            "type": "object", "properties": { "domain": { "type": "string" }, "status": { "type": "integer" } }, "required": ["domain"]
                        }
                    },
                    {
                        "name": "save_endpoint",
                        "description": "Save an endpoint discovered during recon.",
                        "inputSchema": {
                            "type": "object",
                            "properties": {
                                "domain": { "type": "string" }, "method": { "type": "string" }, "path": { "type": "string" },
                                "status_code": { "type": "integer" }, "auth": { "type": "boolean" },
                                "description": { "type": "string" }, "notes": { "type": "string" }
                            },
                            "required": ["domain", "method", "path"]
                        }
                    },
                    {
                        "name": "get_findings",
                        "description": "List vulnerability findings.",
                        "inputSchema": {
                            "type": "object",
                            "properties": { "domain": { "type": "string" }, "severity": { "type": "string" }, "status": { "type": "string" } },
                            "required": ["domain"]
                        }
                    },
                    {
                        "name": "save_finding",
                        "description": "Save a vulnerability finding.",
                        "inputSchema": {
                            "type": "object",
                            "properties": {
                                "domain": { "type": "string" }, "type": { "type": "string" },
                                "severity": { "type": "string", "enum": ["info", "low", "medium", "high", "critical"] },
                                "status": { "type": "string", "enum": ["potential", "confirmed", "false_positive"] },
                                "endpoint_id": { "type": "integer" }, "request_id": { "type": "integer" },
                                "description": { "type": "string" }, "payload": { "type": "string" }, "evidence": { "type": "string" }
                            },
                            "required": ["domain", "type", "severity"]
                        }
                    },
                    {
                        "name": "get_coverage",
                        "description": "Get test coverage for an endpoint.",
                        "inputSchema": { "type": "object", "properties": { "endpoint_id": { "type": "integer" }, "status": { "type": "string" } }, "required": ["endpoint_id"] }
                    },
                    {
                        "name": "upsert_coverage",
                        "description": "Update vector test status for an endpoint.",
                        "inputSchema": {
                            "type": "object",
                            "properties": {
                                "endpoint_id": { "type": "integer" }, "vector": { "type": "string" },
                                "status": { "type": "string", "enum": ["pending", "in_progress", "done", "skipped"] },
                                "description": { "type": "string" }, "notes": { "type": "string" }
                            },
                            "required": ["endpoint_id", "vector", "status"]
                        }
                    },
                    {
                        "name": "save_request",
                        "description": "Save raw HTTP request/response evidence.",
                        "inputSchema": {
                            "type": "object",
                            "properties": {
                                "endpoint_id": { "type": "integer" }, "raw_request": { "type": "string" }, "raw_response": { "type": "string" },
                                "status_code": { "type": "integer" }, "description": { "type": "string" }
                            },
                            "required": ["endpoint_id", "raw_request"]
                        }
                    },
                    {
                        "name": "get_requests",
                        "description": "List saved requests for an endpoint.",
                        "inputSchema": { "type": "object", "properties": { "endpoint_id": { "type": "integer" } }, "required": ["endpoint_id"] }
                    },
                    {
                        "name": "save_credential",
                        "description": "Save discovered credential.",
                        "inputSchema": {
                            "type": "object",
                            "properties": {
                                "domain": { "type": "string" }, "type": { "type": "string" },
                                "username": { "type": "string" }, "secret": { "type": "string" },
                                "description": { "type": "string" }
                            },
                            "required": ["domain", "type", "secret"]
                        }
                    },
                    {
                        "name": "get_credentials",
                        "description": "List credentials.",
                        "inputSchema": { "type": "object", "properties": { "domain": { "type": "string" } }, "required": ["domain"] }
                    },
                    {
                        "name": "get_chains",
                        "description": "List attack chains.",
                        "inputSchema": { "type": "object", "properties": { "domain": { "type": "string" } }, "required": ["domain"] }
                    },
                    {
                        "name": "save_chain",
                        "description": "Create attack chain.",
                        "inputSchema": {
                            "type": "object",
                            "properties": {
                                "domain": { "type": "string" }, "title": { "type": "string" },
                                "severity": { "type": "string" }, "description": { "type": "string" }
                            },
                            "required": ["domain", "title", "severity"]
                        }
                    },
                    {
                        "name": "get_chain_steps",
                        "description": "List steps in an attack chain.",
                        "inputSchema": { "type": "object", "properties": { "chain_id": { "type": "integer" } }, "required": ["chain_id"] }
                    },
                    {
                        "name": "add_chain_step",
                        "description": "Add a finding to a chain.",
                        "inputSchema": {
                            "type": "object",
                            "properties": { "chain_id": { "type": "integer" }, "finding_id": { "type": "integer" }, "step_order": { "type": "integer" }, "notes": { "type": "string" } },
                            "required": ["chain_id", "finding_id", "step_order"]
                        }
                    }
                ]
            }
        }),
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
    let domain = args["domain"].as_str().unwrap_or("");
    let endpoint_id = args["endpoint_id"].as_i64().unwrap_or(0);
    let chain_id = args["chain_id"].as_i64().unwrap_or(0);

    match name {
        "get_summary" => match summary::get(&state.db, domain).await {
            Ok(Some(s)) => serde_json::to_string_pretty(&s).unwrap_or_default(),
            Ok(None) => "Target not found".to_string(),
            Err(e) => format!("Error: {}", e),
        },
        "get_scope" => match scope::get(&state.db, domain).await {
            Ok(Some(s)) => serde_json::to_string_pretty(&s).unwrap_or_default(),
            Ok(None) => "Scope not found".to_string(),
            Err(e) => format!("Error: {}", e),
        },
        "save_scope" => {
            let input = scope::CreateScope {
                objective: args["objective"].as_str().unwrap_or("").to_string(),
                in_scope: args["in_scope"].as_str().unwrap_or("").to_string(),
                out_of_scope: args["out_of_scope"].as_str().map(String::from),
                rules: args["rules"].as_str().map(String::from),
            };
            match scope::upsert(&state.db, domain, &input).await {
                Ok(id) => format!("Scope saved. ID: {}", id),
                Err(e) => format!("Error: {}", e),
            }
        }
        "get_relations" => match target_relations::list(&state.db, domain).await {
            Ok(s) => serde_json::to_string_pretty(&s).unwrap_or_default(),
            Err(e) => format!("Error: {}", e),
        },
        "save_relation" => {
            let from = args["from_domain"].as_str().unwrap_or("");
            let input = target_relations::CreateRelation {
                to_domain: args["to_domain"].as_str().unwrap_or("").to_string(),
                rel_type: args["rel_type"].as_str().unwrap_or("").to_string(),
                description: args["description"].as_str().map(String::from),
            };
            match target_relations::create(&state.db, from, &input).await {
                Ok(id) => format!("Relation saved. ID: {}", id),
                Err(e) => format!("Error: {}", e),
            }
        }
        "get_endpoints" => {
            let status = args["status"].as_i64();
            match endpoints::list(&state.db, domain, status).await {
                Ok(s) => serde_json::to_string_pretty(&s).unwrap_or_default(),
                Err(e) => format!("Error: {}", e),
            }
        }
        "save_endpoint" => {
            let input = endpoints::CreateEndpoint {
                method: args["method"].as_str().unwrap_or("GET").to_string(),
                path: args["path"].as_str().unwrap_or("").to_string(),
                status_code: args["status_code"].as_i64(),
                auth: args["auth"].as_bool(),
                description: args["description"].as_str().map(String::from),
                notes: args["notes"].as_str().map(String::from),
            };
            match endpoints::create(&state.db, domain, &input).await {
                Ok(id) => format!("Endpoint saved. ID: {}", id),
                Err(e) => format!("Error: {}", e),
            }
        }
        "get_findings" => {
            match findings::list(
                &state.db,
                domain,
                args["severity"].as_str(),
                args["status"].as_str(),
            )
            .await
            {
                Ok(s) => serde_json::to_string_pretty(&s).unwrap_or_default(),
                Err(e) => format!("Error: {}", e),
            }
        }
        "save_finding" => {
            let input = findings::CreateFinding {
                endpoint_id: args["endpoint_id"].as_i64(),
                request_id: args["request_id"].as_i64(),
                parent_id: None,
                r#type: args["type"].as_str().unwrap_or("").to_string(),
                severity: args["severity"].as_str().unwrap_or("info").to_string(),
                status: args["status"].as_str().map(String::from),
                raw_request: None,
                payload: args["payload"].as_str().map(String::from),
                evidence: args["evidence"].as_str().map(String::from),
                description: args["description"].as_str().map(String::from),
            };
            match findings::create(&state.db, domain, &input).await {
                Ok(id) => format!("Finding saved. ID: {}", id),
                Err(e) => format!("Error: {}", e),
            }
        }
        "get_coverage" => {
            match coverage::list(&state.db, endpoint_id, args["status"].as_str()).await {
                Ok(s) => serde_json::to_string_pretty(&s).unwrap_or_default(),
                Err(e) => format!("Error: {}", e),
            }
        }
        "upsert_coverage" => {
            let input = coverage::UpsertCoverage {
                vector: args["vector"].as_str().unwrap_or("").to_string(),
                status: args["status"].as_str().unwrap_or("pending").to_string(),
                description: args["description"].as_str().map(String::from),
                notes: args["notes"].as_str().map(String::from),
            };
            match coverage::upsert(&state.db, endpoint_id, &input).await {
                Ok(id) => format!("Coverage updated. ID: {}", id),
                Err(e) => format!("Error: {}", e),
            }
        }
        "get_requests" => match requests::list(&state.db, endpoint_id).await {
            Ok(s) => serde_json::to_string_pretty(&s).unwrap_or_default(),
            Err(e) => format!("Error: {}", e),
        },
        "save_request" => {
            let input = requests::CreateRequest {
                raw_request: args["raw_request"].as_str().unwrap_or("").to_string(),
                raw_response: args["raw_response"].as_str().map(String::from),
                status_code: args["status_code"].as_i64(),
                response_time_ms: None,
                description: args["description"].as_str().map(String::from),
                notes: None,
            };
            match requests::create(&state.db, endpoint_id, &input).await {
                Ok(id) => format!("Request saved. ID: {}", id),
                Err(e) => format!("Error: {}", e),
            }
        }
        "get_credentials" => match credentials::list(&state.db, domain).await {
            Ok(s) => serde_json::to_string_pretty(&s).unwrap_or_default(),
            Err(e) => format!("Error: {}", e),
        },
        "save_credential" => {
            let input = credentials::CreateCredential {
                r#type: args["type"].as_str().unwrap_or("").to_string(),
                username: args["username"].as_str().map(String::from),
                secret: args["secret"].as_str().unwrap_or("").to_string(),
                description: args["description"].as_str().map(String::from),
                notes: None,
            };
            match credentials::create(&state.db, domain, &input).await {
                Ok(id) => format!("Credential saved. ID: {}", id),
                Err(e) => format!("Error: {}", e),
            }
        }
        "get_chains" => match attack_chains::list(&state.db, domain).await {
            Ok(s) => serde_json::to_string_pretty(&s).unwrap_or_default(),
            Err(e) => format!("Error: {}", e),
        },
        "save_chain" => {
            let input = attack_chains::CreateChain {
                title: args["title"].as_str().unwrap_or("").to_string(),
                description: args["description"].as_str().map(String::from),
                severity: args["severity"].as_str().unwrap_or("info").to_string(),
            };
            match attack_chains::create(&state.db, domain, &input).await {
                Ok(id) => format!("Chain saved. ID: {}", id),
                Err(e) => format!("Error: {}", e),
            }
        }
        "get_chain_steps" => match attack_chains::steps(&state.db, chain_id).await {
            Ok(s) => serde_json::to_string_pretty(&s).unwrap_or_default(),
            Err(e) => format!("Error: {}", e),
        },
        "add_chain_step" => {
            let input = attack_chains::AddStep {
                finding_id: args["finding_id"].as_i64().unwrap_or(0),
                step_order: args["step_order"].as_i64().unwrap_or(0),
                notes: args["notes"].as_str().map(String::from),
            };
            match attack_chains::add_step(&state.db, chain_id, &input).await {
                Ok(id) => format!("Chain step added. ID: {}", id),
                Err(e) => format!("Error: {}", e),
            }
        }
        _ => format!("Unknown tool: {}", name),
    }
}
