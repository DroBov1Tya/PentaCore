use crate::app::database::queries::endpoints::{self, CreateEndpoint};
use serde_json::Value;
use sqlx::SqlitePool;
use std::collections::HashMap;

pub async fn parse_and_import_openapi(
    db: &SqlitePool,
    domain: &str,
    spec_json: &str,
) -> anyhow::Result<String> {
    let parsed: Value = serde_json::from_str(spec_json)?;
    let mut imported = 0;
    let mut methods_count: HashMap<String, u32> = HashMap::new();

    let paths = parsed.get("paths").and_then(|p| p.as_object());
    
    if let Some(paths) = paths {
        for (path, path_item) in paths {
            if let Some(methods) = path_item.as_object() {
                for (method, operation) in methods {
                    let method_upper = method.to_uppercase();
                    
                    if method_upper.starts_with("X-") || method_upper == "PARAMETERS" || method_upper == "SERVERS" {
                        continue;
                    }

                    let summary = operation
                        .get("summary")
                        .and_then(|s| s.as_str())
                        .unwrap_or("")
                        .to_string();

                    let mut params_info = Vec::new();
                    if let Some(params) = operation.get("parameters").and_then(|p| p.as_array()) {
                        for p in params {
                            if let Some(name) = p.get("name").and_then(|n| n.as_str()) {
                                let in_loc = p.get("in").and_then(|i| i.as_str()).unwrap_or("unknown");
                                params_info.push(format!("{} ({})", name, in_loc));
                            }
                        }
                    }

                    let mut notes = String::new();
                    if !params_info.is_empty() {
                        notes.push_str(&format!("Params: {}\n", params_info.join(", ")));
                    }

                    let auth = operation.get("security").is_some();

                    let input = CreateEndpoint {
                        method: method_upper.clone(),
                        path: path.clone(),
                        status_code: None,
                        auth: Some(auth),
                        description: if summary.is_empty() { None } else { Some(summary) },
                        notes: if notes.is_empty() { None } else { Some(notes) },
                    };

                    match endpoints::create(db, domain, &input).await {
                        Ok(_) => {
                            imported += 1;
                            *methods_count.entry(method_upper).or_insert(0) += 1;
                        }
                        Err(_) => {
                        }
                    }
                }
            }
        }
    } else {
        return Err(anyhow::anyhow!("No 'paths' object found in the provided JSON. Make sure it's a valid OpenAPI/Swagger schema."));
    }

    let mut summary = format!("Successfully imported {} new endpoints.\n", imported);
    for (m, c) in methods_count {
        summary.push_str(&format!("{}: {}\n", m, c));
    }

    Ok(summary)
}
