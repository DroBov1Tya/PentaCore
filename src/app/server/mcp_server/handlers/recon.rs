use crate::app::AppState;
use crate::app::database::queries::{endpoints, target_relations};
use serde_json::Value;
use std::sync::Arc;

pub async fn handle_get_relations(args: &Value, state: &Arc<AppState>) -> String {
    let domain = args["domain"].as_str().unwrap_or("");
    match target_relations::list(&state.db, domain).await {
        Ok(s) => serde_json::to_string_pretty(&s).unwrap_or_default(),
        Err(e) => format!("Error: {}", e),
    }
}

pub async fn handle_save_relation(args: &Value, state: &Arc<AppState>) -> String {
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

pub async fn handle_get_endpoints(args: &Value, state: &Arc<AppState>) -> String {
    let domain = args["domain"].as_str().unwrap_or("");
    let status = args["status"].as_i64();
    match endpoints::list(&state.db, domain, status).await {
        Ok(s) => serde_json::to_string_pretty(&s).unwrap_or_default(),
        Err(e) => format!("Error: {}", e),
    }
}

pub async fn handle_save_endpoint(args: &Value, state: &Arc<AppState>) -> String {
    let domain = args["domain"].as_str().unwrap_or("");
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

pub async fn handle_enumerate_subdomains(args: &Value, state: &Arc<AppState>) -> String {
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

pub async fn handle_resolve_dns(args: &Value, _state: &Arc<AppState>) -> String {
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
