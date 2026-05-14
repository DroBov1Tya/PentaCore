use anyhow::Result;
use serde_json::{Value, json};
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, BufReader};

use crate::app::AppState;
use crate::app::database::queries::{
    attack_chains, coverage, credentials, endpoint_examples, endpoints, findings, requests, scope,
    summary, target_relations, test_objects,
};

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
        "set_session" => {
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
        "revoke_session" => match crate::app::client::sessions::remove_session().await {
            Ok(_) => "Session state revoked successfully.".to_string(),
            Err(e) => format!("Error revoking session: {}", e),
        },
        "make_request" => {
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

                    if let Some(recon_hint) =
                        crate::app::client::recon::analyze_response(&resp_text)
                    {
                        hint.push_str(&recon_hint);
                    }

                    format!("Status: {}{}\n\n{}", status, hint, resp_text)
                }
                Err(e) => format!("Request error: {}", e),
            }
        }
        "resolve_dns" => {
            let domain = args["domain"].as_str().unwrap_or("").to_string();
            if domain.is_empty() {
                return "Error: domain is required".to_string();
            }
            let results = crate::app::client::dns::resolve_dns(&domain).await;
            format!(
                "DNS Resolution Results for {}:\n{}",
                domain,
                results.join("\n")
            )
        }
        "enumerate_subdomains" => {
            let domain = args["domain"].as_str().unwrap_or("").to_string();
            if domain.is_empty() {
                return "Error: domain is required".to_string();
            }

            let results = crate::app::client::dns::enumerate_subdomains(&domain).await;

            for res in &results {
                if let Some(sub) = res.split(" ->").next() {
                    let rel = target_relations::CreateRelation {
                        to_domain: sub.to_string(),
                        rel_type: "subdomain".to_string(),
                        description: Some("Discovered via enumerate_subdomains".to_string()),
                    };
                    let _ = target_relations::create(&state.db, &domain, &rel).await;
                }
            }

            format!(
                "Subdomain Enumeration Results:\nFound {} subdomains:\n{}",
                results.len(),
                results.join("\n")
            )
        }
        "make_race_requests" => {
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
        "diff_requests" => {
            let id_a = args["request_id_a"].as_i64().unwrap_or(0);
            let id_b = args["request_id_b"].as_i64().unwrap_or(0);
            if id_a == 0 || id_b == 0 {
                return "Error: request_id_a and request_id_b are required".to_string();
            }

            let req_a =
                sqlx::query_as::<_, requests::RequestRow>("SELECT * FROM requests WHERE id = ?")
                    .bind(id_a)
                    .fetch_optional(&state.db)
                    .await;

            let req_b =
                sqlx::query_as::<_, requests::RequestRow>("SELECT * FROM requests WHERE id = ?")
                    .bind(id_b)
                    .fetch_optional(&state.db)
                    .await;

            match (req_a, req_b) {
                (Ok(Some(a)), Ok(Some(b))) => {
                    let mut diff = Vec::new();

                    // Status code
                    match (a.status_code, b.status_code) {
                        (Some(sa), Some(sb)) if sa != sb => {
                            diff.push(format!("STATUS: {} vs {}", sa, sb))
                        }
                        _ => {}
                    }

                    // Response size
                    let size_a = a.raw_response.as_ref().map(|r| r.len()).unwrap_or(0);
                    let size_b = b.raw_response.as_ref().map(|r| r.len()).unwrap_or(0);
                    if size_a != size_b {
                        diff.push(format!(
                            "SIZE: {} bytes vs {} bytes (delta: {})",
                            size_a,
                            size_b,
                            (size_b as i64 - size_a as i64)
                        ));
                    }

                    // Response time
                    match (a.response_time_ms, b.response_time_ms) {
                        (Some(ta), Some(tb)) if ta != tb => {
                            diff.push(format!("TIME: {}ms vs {}ms", ta, tb))
                        }
                        _ => {}
                    }

                    // JSON structure diff
                    let body_a = a.raw_response.as_deref().unwrap_or("");
                    let body_b = b.raw_response.as_deref().unwrap_or("");
                    if let (Ok(ja), Ok(jb)) = (
                        serde_json::from_str::<Value>(body_a),
                        serde_json::from_str::<Value>(body_b),
                    ) {
                        if let (Some(oa), Some(ob)) = (ja.as_object(), jb.as_object()) {
                            let keys_a: std::collections::HashSet<_> = oa.keys().collect();
                            let keys_b: std::collections::HashSet<_> = ob.keys().collect();
                            let only_a: Vec<_> = keys_a.difference(&keys_b).collect();
                            let only_b: Vec<_> = keys_b.difference(&keys_a).collect();
                            if !only_a.is_empty() {
                                diff.push(format!("JSON keys only in A: {:?}", only_a));
                            }
                            if !only_b.is_empty() {
                                diff.push(format!("JSON keys only in B: {:?}", only_b));
                            }
                            for key in keys_a.intersection(&keys_b) {
                                if oa[*key] != ob[*key] {
                                    diff.push(format!("JSON field '{}' differs", key));
                                }
                            }
                        }
                    }

                    if diff.is_empty() {
                        format!(
                            "Diff: Responses are identical (request {} vs {})",
                            id_a, id_b
                        )
                    } else {
                        format!("Diff (request {} vs {}):\n{}", id_a, id_b, diff.join("\n"))
                    }
                }
                _ => "Error: one or both request IDs not found".to_string(),
            }
        }
        "claim_test_object" => {
            let input = test_objects::ClaimTestObject {
                object_type: args["object_type"].as_str().unwrap_or("").to_string(),
                object_id: args["object_id"].as_str().unwrap_or("").to_string(),
                description: args["description"].as_str().map(String::from),
                rollback_method: args["rollback_method"].as_str().map(String::from),
                rollback_url: args["rollback_url"].as_str().map(String::from),
                rollback_body: args["rollback_body"].as_str().map(String::from),
            };
            match test_objects::claim(&state.db, domain, &input).await {
                Ok(id) => format!(
                    "Test object claimed. ID: {}. Remember to rollback after testing.",
                    id
                ),
                Err(e) => format!("Error: {}", e),
            }
        }
        "rollback_test_object" => {
            let obj_id = args["id"].as_i64().unwrap_or(0);
            if obj_id == 0 {
                return "Error: id is required".to_string();
            }

            match test_objects::rollback(&state.db, obj_id).await {
                Ok(Some(obj)) => {
                    if obj.rollback_method.is_some() && obj.rollback_url.is_some() {
                        let prereq = crate::app::client::req::PreRequest {
                            cookie: vec![],
                            method: obj.rollback_method.unwrap(),
                            url: obj.rollback_url.unwrap(),
                            body: obj.rollback_body.unwrap_or_default(),
                            proxy: None,
                            user_agent: None,
                        };
                        match crate::app::client::req::make_req(prereq).await {
                            Ok(resp) => format!(
                                "Rollback executed. HTTP {}. Object {} marked as rolled_back.",
                                resp.status().as_u16(),
                                obj_id
                            ),
                            Err(e) => format!(
                                "Rollback request failed: {}. Object marked as rolled_back anyway.",
                                e
                            ),
                        }
                    } else {
                        format!(
                            "Object {} marked as rolled_back. No rollback URL was configured — manual cleanup may be needed.",
                            obj_id
                        )
                    }
                }
                Ok(None) => format!("Error: test object with id {} not found", obj_id),
                Err(e) => format!("Error: {}", e),
            }
        }
        "get_test_objects" => {
            let status_filter = args["status"].as_str();
            match test_objects::list(&state.db, domain, status_filter).await {
                Ok(s) => serde_json::to_string_pretty(&s).unwrap_or_default(),
                Err(e) => format!("Error: {}", e),
            }
        }
        "bulk_upsert_coverage" => {
            let items = match args["items"].as_array() {
                Some(arr) => arr,
                None => return "Error: 'items' must be an array of {endpoint_id, vector, status, description?, notes?}".to_string(),
            };
            let mut ok = 0u32;
            let mut errors = Vec::new();
            for item in items {
                let eid = item["endpoint_id"].as_i64().unwrap_or(0);
                let input = coverage::UpsertCoverage {
                    vector: item["vector"].as_str().unwrap_or("").to_string(),
                    status: item["status"].as_str().unwrap_or("pending").to_string(),
                    description: item["description"].as_str().map(String::from),
                    notes: item["notes"].as_str().map(String::from),
                };
                match coverage::upsert(&state.db, eid, &input).await {
                    Ok(_) => ok += 1,
                    Err(e) => errors.push(format!("endpoint_id {}: {}", eid, e)),
                }
            }
            if errors.is_empty() {
                format!("Bulk coverage: {} items upserted successfully.", ok)
            } else {
                format!(
                    "Bulk coverage: {} ok, {} errors:\n{}",
                    ok,
                    errors.len(),
                    errors.join("\n")
                )
            }
        }
        "bulk_save_requests" => {
            let items = match args["items"].as_array() {
                Some(arr) => arr,
                None => return "Error: 'items' must be an array of {endpoint_id, raw_request, raw_response?, status_code?, description?}".to_string(),
            };
            let mut ok = 0u32;
            let mut ids = Vec::new();
            let mut errors = Vec::new();
            for item in items {
                let eid = item["endpoint_id"].as_i64().unwrap_or(0);
                let input = requests::CreateRequest {
                    raw_request: item["raw_request"].as_str().unwrap_or("").to_string(),
                    raw_response: item["raw_response"].as_str().map(String::from),
                    status_code: item["status_code"].as_i64(),
                    response_time_ms: item["response_time_ms"].as_i64(),
                    description: item["description"].as_str().map(String::from),
                    notes: item["notes"].as_str().map(String::from),
                };
                match requests::create(&state.db, eid, &input).await {
                    Ok(id) => {
                        ok += 1;
                        ids.push(id.to_string());
                    }
                    Err(e) => errors.push(format!("endpoint_id {}: {}", eid, e)),
                }
            }
            if errors.is_empty() {
                format!("Bulk requests: {} saved. IDs: [{}]", ok, ids.join(", "))
            } else {
                format!(
                    "Bulk requests: {} ok, {} errors:\n{}",
                    ok,
                    errors.len(),
                    errors.join("\n")
                )
            }
        }
        "save_endpoint_example" => {
            let input = endpoint_examples::SaveExample {
                raw_request: args["raw_request"].as_str().unwrap_or("").to_string(),
                raw_response: args["raw_response"].as_str().map(String::from),
                status_code: args["status_code"].as_i64(),
                description: args["description"].as_str().map(String::from),
            };
            match endpoint_examples::upsert(&state.db, endpoint_id, &input).await {
                Ok(id) => format!("Endpoint example saved. ID: {}", id),
                Err(e) => format!("Error: {}", e),
            }
        }
        "get_endpoint_example" => match endpoint_examples::get(&state.db, endpoint_id).await {
            Ok(Some(ex)) => serde_json::to_string_pretty(&ex).unwrap_or_default(),
            Ok(None) => "No example saved for this endpoint yet.".to_string(),
            Err(e) => format!("Error: {}", e),
        },
        "parse_api_spec" => {
            let spec_url = args["url"].as_str().unwrap_or("");
            let spec_json = args["json"].as_str().unwrap_or("");

            if spec_url.is_empty() && spec_json.is_empty() {
                return "Error: either 'url' or 'json' must be provided.".to_string();
            }

            let json_content = if !spec_url.is_empty() {
                let prereq = crate::app::client::req::PreRequest {
                    cookie: vec![],
                    method: "GET".to_string(),
                    url: spec_url.to_string(),
                    body: "".to_string(),
                    proxy: None,
                    user_agent: None,
                };
                match crate::app::client::req::make_req(prereq).await {
                    Ok(resp) => {
                        match resp.text().await {
                            Ok(text) => text,
                            Err(e) => return format!("Error reading response body from URL: {}", e),
                        }
                    }
                    Err(e) => return format!("Error downloading spec from URL: {}", e),
                }
            } else {
                spec_json.to_string()
            };

            match crate::app::client::api_parser::parse_and_import_openapi(&state.db, domain, &json_content).await {
                Ok(summary) => summary,
                Err(e) => format!("Failed to parse API spec: {}", e),
            }
        }
        _ => format!("Unknown tool: {}", name),
    }
}
