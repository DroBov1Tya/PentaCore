use crate::app::AppState;
use crate::app::database::queries::endpoint_examples;
use serde_json::Value;
use std::sync::Arc;

pub async fn handle_save_endpoint_example(args: &Value, state: &Arc<AppState>) -> String {
    let endpoint_id = args["endpoint_id"].as_i64().unwrap_or(0);
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

pub async fn handle_get_endpoint_example(args: &Value, state: &Arc<AppState>) -> String {
    let endpoint_id = args["endpoint_id"].as_i64().unwrap_or(0);
    match endpoint_examples::get(&state.db, endpoint_id).await {
        Ok(Some(ex)) => serde_json::to_string_pretty(&ex).unwrap_or_default(),
        Ok(None) => "No example saved for this endpoint yet.".to_string(),
        Err(e) => format!("Error: {}", e),
    }
}

pub async fn handle_parse_api_spec(args: &Value, state: &Arc<AppState>) -> String {
    let domain = args["domain"].as_str().unwrap_or("");
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
            http_version: None,
            custom_headers: None,
        };
        match crate::app::client::req::make_req(prereq).await {
            Ok(resp) => match resp.text().await {
                Ok(text) => text,
                Err(e) => return format!("Error reading response body from URL: {}", e),
            },
            Err(e) => return format!("Error downloading spec from URL: {}", e),
        }
    } else {
        spec_json.to_string()
    };

    match crate::app::client::api_parser::parse_and_import_openapi(&state.db, domain, &json_content)
        .await
    {
        Ok(summary) => summary,
        Err(e) => format!("Failed to parse API spec: {}", e),
    }
}

pub async fn handle_parse_graphql_spec(args: &Value, state: &Arc<AppState>) -> String {
    let domain = args["domain"].as_str().unwrap_or("");
    let spec_url = args["url"].as_str().unwrap_or("");
    let spec_json = args["json"].as_str().unwrap_or("");
    let base_endpoint = args["base_endpoint"].as_str().unwrap_or("/graphql");

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
            http_version: None,
            custom_headers: None,
        };
        match crate::app::client::req::make_req(prereq).await {
            Ok(resp) => match resp.text().await {
                Ok(text) => text,
                Err(e) => return format!("Error reading response body from URL: {}", e),
            },
            Err(e) => return format!("Error downloading GraphQL spec from URL: {}", e),
        }
    } else {
        spec_json.to_string()
    };

    match crate::app::client::api_parser::parse_and_import_graphql(
        &state.db,
        domain,
        &json_content,
        base_endpoint,
    )
    .await
    {
        Ok(summary) => summary,
        Err(e) => format!("Failed to parse GraphQL spec: {}", e),
    }
}
