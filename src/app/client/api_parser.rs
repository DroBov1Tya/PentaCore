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

                    if method_upper.starts_with("X-")
                        || method_upper == "PARAMETERS"
                        || method_upper == "SERVERS"
                    {
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
                                let in_loc =
                                    p.get("in").and_then(|i| i.as_str()).unwrap_or("unknown");
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
                        description: if summary.is_empty() {
                            None
                        } else {
                            Some(summary)
                        },
                        notes: if notes.is_empty() { None } else { Some(notes) },
                    };

                    match endpoints::create(db, domain, &input).await {
                        Ok(_) => {
                            imported += 1;
                            *methods_count.entry(method_upper).or_insert(0) += 1;
                        }
                        Err(_) => {}
                    }
                }
            }
        }
    } else {
        return Err(anyhow::anyhow!(
            "No 'paths' object found in the provided JSON. Make sure it's a valid OpenAPI/Swagger schema."
        ));
    }

    let mut summary = format!("Successfully imported {} new endpoints.\n", imported);
    for (m, c) in methods_count {
        summary.push_str(&format!("{}: {}\n", m, c));
    }

    Ok(summary)
}

pub async fn parse_and_import_graphql(
    db: &SqlitePool,
    domain: &str,
    spec_json: &str,
    base_endpoint: &str,
) -> anyhow::Result<String> {
    let parsed: Value = serde_json::from_str(spec_json)?;
    let mut imported = 0;
    let mut types_count: HashMap<String, u32> = HashMap::new();

    let schema = parsed
        .get("data")
        .and_then(|d| d.get("__schema"))
        .or_else(|| parsed.get("__schema"));

    if let Some(schema) = schema {
        let query_type = schema
            .get("queryType")
            .and_then(|t| t.get("name"))
            .and_then(|n| n.as_str())
            .unwrap_or("Query");
        let mutation_type = schema
            .get("mutationType")
            .and_then(|t| t.get("name"))
            .and_then(|n| n.as_str())
            .unwrap_or("Mutation");

        if let Some(types) = schema.get("types").and_then(|t| t.as_array()) {
            for type_info in types {
                let name = type_info.get("name").and_then(|n| n.as_str()).unwrap_or("");
                if name.starts_with("__") {
                    continue;
                }

                let is_query = name == query_type;
                let is_mutation = name == mutation_type;

                if is_query || is_mutation {
                    let op_type = if is_query { "QUERY" } else { "MUTATION" };

                    if let Some(fields) = type_info.get("fields").and_then(|f| f.as_array()) {
                        for field in fields {
                            let field_name =
                                field.get("name").and_then(|n| n.as_str()).unwrap_or("");
                            let description = field
                                .get("description")
                                .and_then(|d| d.as_str())
                                .unwrap_or("")
                                .to_string();

                            let mut args_info = Vec::new();
                            if let Some(args) = field.get("args").and_then(|a| a.as_array()) {
                                for arg in args {
                                    if let Some(arg_name) = arg.get("name").and_then(|n| n.as_str())
                                    {
                                        let mut type_str = String::from("unknown");
                                        if let Some(type_obj) = arg.get("type") {
                                            if let Some(t_name) =
                                                type_obj.get("name").and_then(|n| n.as_str())
                                            {
                                                type_str = t_name.to_string();
                                            } else if let Some(of_type) = type_obj.get("ofType") {
                                                if let Some(t_name) =
                                                    of_type.get("name").and_then(|n| n.as_str())
                                                {
                                                    type_str = format!("[{}]", t_name);
                                                }
                                            }
                                        }
                                        args_info.push(format!("{}: {}", arg_name, type_str));
                                    }
                                }
                            }

                            let mut notes = format!("GraphQL {}\n", op_type);
                            if !args_info.is_empty() {
                                notes.push_str(&format!("Args: {}\n", args_info.join(", ")));
                            }

                            let path = if base_endpoint.is_empty() {
                                format!("/graphql?{}", field_name)
                            } else {
                                format!("{}?{}", base_endpoint, field_name)
                            };

                            let input = CreateEndpoint {
                                method: "POST".to_string(),
                                path,
                                status_code: None,
                                auth: None,
                                description: if description.is_empty() {
                                    None
                                } else {
                                    Some(description)
                                },
                                notes: Some(notes),
                            };

                            match endpoints::create(db, domain, &input).await {
                                Ok(_) => {
                                    imported += 1;
                                    *types_count.entry(op_type.to_string()).or_insert(0) += 1;
                                }
                                Err(_) => {}
                            }
                        }
                    }
                }
            }
        }
    } else {
        return Err(anyhow::anyhow!(
            "No '__schema' object found. Make sure it's a valid GraphQL Introspection JSON."
        ));
    }

    let mut summary = format!("Successfully imported {} GraphQL operations.\n", imported);
    for (m, c) in types_count {
        summary.push_str(&format!("{}: {}\n", m, c));
    }

    Ok(summary)
}
