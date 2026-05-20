use crate::app::AppState;
use crate::app::database::queries::report;
use serde_json::Value;
use std::sync::Arc;

pub async fn handle_generate_report(args: &Value, state: &Arc<AppState>) -> String {
    let domain = args["domain"].as_str().unwrap_or("");
    match report::generate(&state.db, domain).await {
        Ok(result) => serde_json::to_string_pretty(&result).unwrap_or_default(),
        Err(e) => format!("Error: {}", e),
    }
}
