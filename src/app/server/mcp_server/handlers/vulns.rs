use crate::app::AppState;
use crate::app::database::queries::{attack_chains, credentials, findings};
use serde_json::Value;
use std::sync::Arc;

pub async fn handle_get_findings(args: &Value, state: &Arc<AppState>) -> String {
    let domain = args["domain"].as_str().unwrap_or("");
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

pub async fn handle_save_finding(args: &Value, state: &Arc<AppState>) -> String {
    let domain = args["domain"].as_str().unwrap_or("");
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

pub async fn handle_get_credentials(args: &Value, state: &Arc<AppState>) -> String {
    let domain = args["domain"].as_str().unwrap_or("");
    match credentials::list(&state.db, domain).await {
        Ok(s) => serde_json::to_string_pretty(&s).unwrap_or_default(),
        Err(e) => format!("Error: {}", e),
    }
}

pub async fn handle_save_credential(args: &Value, state: &Arc<AppState>) -> String {
    let domain = args["domain"].as_str().unwrap_or("");
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

pub async fn handle_get_chains(args: &Value, state: &Arc<AppState>) -> String {
    let domain = args["domain"].as_str().unwrap_or("");
    match attack_chains::list(&state.db, domain).await {
        Ok(s) => serde_json::to_string_pretty(&s).unwrap_or_default(),
        Err(e) => format!("Error: {}", e),
    }
}

pub async fn handle_save_chain(args: &Value, state: &Arc<AppState>) -> String {
    let domain = args["domain"].as_str().unwrap_or("");
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

pub async fn handle_get_chain_steps(args: &Value, state: &Arc<AppState>) -> String {
    let chain_id = args["chain_id"].as_i64().unwrap_or(0);
    match attack_chains::steps(&state.db, chain_id).await {
        Ok(s) => serde_json::to_string_pretty(&s).unwrap_or_default(),
        Err(e) => format!("Error: {}", e),
    }
}

pub async fn handle_add_chain_step(args: &Value, state: &Arc<AppState>) -> String {
    let chain_id = args["chain_id"].as_i64().unwrap_or(0);
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
