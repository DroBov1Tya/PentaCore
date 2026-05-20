use crate::app::AppState;
use crate::app::database::queries::{attack_chains, credentials, findings};
use serde_json::Value;
use std::sync::Arc;

/// Retrieves a list of discovered vulnerabilities (findings) for a target domain.
/// Can be filtered by severity or status to focus on specific findings.
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

/// Registers a new vulnerability finding in the database.
/// Incorporates a Quality Gate: if the finding is marked as 'confirmed',
/// it strictly requires valid evidence (e.g., PoC, response body) to be provided.
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

    if input.status.as_deref() == Some("confirmed")
        && input.evidence.as_deref().unwrap_or("").trim().is_empty()
    {
        return "QUALITY GATE FAILED: Cannot mark a finding as 'confirmed' without providing 'evidence'. Please provide proof of exploitation.".to_string();
    }

    match findings::create(&state.db, domain, &input).await {
        Ok(id) => format!("Finding saved. ID: {}", id),
        Err(e) => format!("Error: {}", e),
    }
}

/// Updates the metadata (status, severity, evidence, description) of an existing finding.
/// Incorporates the same Quality Gate: escalating status to 'confirmed' requires evidence.
pub async fn handle_update_finding(args: &Value, state: &Arc<AppState>) -> String {
    let id = match args["id"].as_i64() {
        Some(i) => i,
        None => return "Error: finding id is required".to_string(),
    };

    let input = findings::UpdateFinding {
        severity: args["severity"].as_str().map(String::from),
        status: args["status"].as_str().map(String::from),
        evidence: args["evidence"].as_str().map(String::from),
        description: args["description"].as_str().map(String::from),
    };

    if input.status.as_deref() == Some("confirmed") {
        if input.evidence.as_deref().unwrap_or("").trim().is_empty() {
            return "QUALITY GATE FAILED: Cannot mark a finding as 'confirmed' without providing 'evidence' in the update.".to_string();
        }
    }

    match findings::update(&state.db, id, &input).await {
        Ok(true) => format!("Successfully updated finding {}", id),
        Ok(false) => format!("Finding {} not found or no changes made", id),
        Err(e) => format!("Error updating finding: {}", e),
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
