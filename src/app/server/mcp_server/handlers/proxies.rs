use crate::app::database::queries::proxies;
use crate::app::AppState;
use serde_json::Value;
use std::sync::Arc;

pub async fn handle_get_proxies(args: &Value, state: &Arc<AppState>) -> String {
    let domain = args["domain"].as_str().unwrap_or("");
    match proxies::list(&state.db, domain).await {
        Ok(s) => serde_json::to_string_pretty(&s).unwrap_or_default(),
        Err(e) => format!("Error: {}", e),
    }
}

pub async fn handle_save_proxy(args: &Value, state: &Arc<AppState>) -> String {
    let domain = args["domain"].as_str().unwrap_or("");
    let input = proxies::CreateProxy {
        url: args["url"].as_str().unwrap_or("").to_string(),
        r#type: args["type"].as_str().unwrap_or("http").to_string(),
        active: args["active"].as_i64(),
        description: args["description"].as_str().map(String::from),
        notes: args["notes"].as_str().map(String::from),
    };
    match proxies::create(&state.db, domain, &input).await {
        Ok(id) => format!("Proxy saved. ID: {}", id),
        Err(e) => format!("Error: {}", e),
    }
}
