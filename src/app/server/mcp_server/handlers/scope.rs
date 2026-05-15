use crate::app::AppState;
use crate::app::database::queries::{scope, summary};
use serde_json::Value;
use std::sync::Arc;

pub async fn handle_get_summary(args: &Value, state: &Arc<AppState>) -> String {
    let domain = args["domain"].as_str().unwrap_or("");
    match summary::get(&state.db, domain).await {
        Ok(Some(s)) => serde_json::to_string_pretty(&s).unwrap_or_default(),
        Ok(None) => "Target not found".to_string(),
        Err(e) => format!("Error: {}", e),
    }
}

pub async fn handle_get_scope(args: &Value, state: &Arc<AppState>) -> String {
    let domain = args["domain"].as_str().unwrap_or("");
    match scope::get(&state.db, domain).await {
        Ok(Some(s)) => serde_json::to_string_pretty(&s).unwrap_or_default(),
        Ok(None) => "Scope not found".to_string(),
        Err(e) => format!("Error: {}", e),
    }
}

pub async fn handle_save_scope(args: &Value, state: &Arc<AppState>) -> String {
    let domain = args["domain"].as_str().unwrap_or("");
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
