use crate::app::AppState;
use crate::app::database::queries::test_objects;
use serde_json::Value;
use std::sync::Arc;

/// Registers a newly created artifact (like a mock user, test post, or API key) in the database.
/// If `rollback_url` and `rollback_method` are provided, the object can be automatically cleaned up later.
pub async fn handle_claim_test_object(args: &Value, state: &Arc<AppState>) -> String {
    let domain = args["domain"].as_str().unwrap_or("");
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

/// Automates the cleanup of a claimed test object.
/// If the object has a rollback HTTP configuration, it executes the HTTP request
/// (e.g., sending a DELETE request to an API endpoint) before marking the object as rolled back.
pub async fn handle_rollback_test_object(args: &Value, state: &Arc<AppState>) -> String {
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
                    "Object {} marked as rolled_back. No rollback URL was configured.",
                    obj_id
                )
            }
        }
        Ok(None) => format!("Error: test object with id {} not found", obj_id),
        Err(e) => format!("Error: {}", e),
    }
}

/// Retrieves a list of active or rolled-back test objects for a specific domain.
/// Used for tracking what test data was created during an audit to ensure complete cleanup.
pub async fn handle_get_test_objects(args: &Value, state: &Arc<AppState>) -> String {
    let domain = args["domain"].as_str().unwrap_or("");
    let status_filter = args["status"].as_str();
    match test_objects::list(&state.db, domain, status_filter).await {
        Ok(s) => serde_json::to_string_pretty(&s).unwrap_or_default(),
        Err(e) => format!("Error: {}", e),
    }
}
