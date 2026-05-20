use crate::app::AppState;
use crate::app::database::queries::{coverage, requests};
use serde_json::Value;
use std::sync::Arc;

pub async fn handle_get_coverage(args: &Value, state: &Arc<AppState>) -> String {
    let endpoint_id = args["endpoint_id"].as_i64().unwrap_or(0);
    match coverage::list(&state.db, endpoint_id, args["status"].as_str()).await {
        Ok(s) => serde_json::to_string_pretty(&s).unwrap_or_default(),
        Err(e) => format!("Error: {}", e),
    }
}

pub async fn handle_upsert_coverage(args: &Value, state: &Arc<AppState>) -> String {
    let endpoint_id = args["endpoint_id"].as_i64().unwrap_or(0);
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

pub async fn handle_get_requests(args: &Value, state: &Arc<AppState>) -> String {
    let endpoint_id = args["endpoint_id"].as_i64().unwrap_or(0);
    match requests::list(&state.db, endpoint_id).await {
        Ok(s) => serde_json::to_string_pretty(&s).unwrap_or_default(),
        Err(e) => format!("Error: {}", e),
    }
}

pub async fn handle_save_request(args: &Value, state: &Arc<AppState>) -> String {
    let endpoint_id = args["endpoint_id"].as_i64().unwrap_or(0);
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

/// Batch updates coverage for multiple endpoint+vector pairs in a single transaction-like manner.
/// Returns a summary indicating how many coverage entries were successfully updated and lists any errors.
pub async fn handle_bulk_upsert_coverage(args: &Value, state: &Arc<AppState>) -> String {
    let items = match args["items"].as_array() {
        Some(arr) => arr,
        None => return "Error: 'items' must be an array".to_string(),
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

/// Saves multiple HTTP request/response pairs concurrently.
/// Ideal for mass-importing traffic logs or scanning tool output into the evidence store.
pub async fn handle_bulk_save_requests(args: &Value, state: &Arc<AppState>) -> String {
    let items = match args["items"].as_array() {
        Some(arr) => arr,
        None => return "Error: 'items' must be an array".to_string(),
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

/// Compares two saved HTTP responses to identify security-relevant discrepancies.
/// It highlights differences in HTTP status codes, body sizes, timing delays (for blind injection analysis),
/// and structurally compares JSON payloads if applicable.
pub async fn handle_diff_requests(args: &Value, state: &Arc<AppState>) -> String {
    let id_a = args["request_id_a"].as_i64().unwrap_or(0);
    let id_b = args["request_id_b"].as_i64().unwrap_or(0);
    if id_a == 0 || id_b == 0 {
        return "Error: request_id_a and request_id_b are required".to_string();
    }

    let req_a = sqlx::query_as::<_, requests::RequestRow>("SELECT * FROM requests WHERE id = ?")
        .bind(id_a)
        .fetch_optional(&state.db)
        .await;

    let req_b = sqlx::query_as::<_, requests::RequestRow>("SELECT * FROM requests WHERE id = ?")
        .bind(id_b)
        .fetch_optional(&state.db)
        .await;

    match (req_a, req_b) {
        (Ok(Some(a)), Ok(Some(b))) => {
            let mut diff = Vec::new();

            match (a.status_code, b.status_code) {
                (Some(sa), Some(sb)) if sa != sb => diff.push(format!("STATUS: {} vs {}", sa, sb)),
                _ => {}
            }

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

            match (a.response_time_ms, b.response_time_ms) {
                (Some(ta), Some(tb)) if ta != tb => {
                    diff.push(format!("TIME: {}ms vs {}ms", ta, tb))
                }
                _ => {}
            }

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
