use crate::app::AppState;
use crate::app::database::queries::agents;
use serde_json::Value;
use std::sync::Arc;

pub async fn handle_spawn_agent(args: &Value, state: &Arc<AppState>) -> String {
    let domain = args["domain"].as_str().unwrap_or("");
    let role = args["role"].as_str().unwrap_or("");
    let objective = args["objective"].as_str().unwrap_or("");

    if domain.is_empty() || role.is_empty() || objective.is_empty() {
        return "Error: domain, role, and objective are required".to_string();
    }

    match agents::spawn(&state.db, domain, role, objective).await {
        Ok(id) => format!(
            "Agent spawned. ID: {id}\n\nInclude this in the sub-agent prompt:\n\
            Agent ID: {id}\n\
            When done, call update_agent_status(id: \"{id}\", status: \"done\", summary: \"<what you found>\", \
            artifact_ids: [\"<hypothesis/finding IDs you created>\"])"
        ),
        Err(e) => format!("Error spawning agent: {e}"),
    }
}

pub async fn handle_update_agent_status(args: &Value, state: &Arc<AppState>) -> String {
    let id = args["id"].as_str().unwrap_or("");
    let status = args["status"].as_str().unwrap_or("");

    if id.is_empty() || status.is_empty() {
        return "Error: id and status are required".to_string();
    }

    if !matches!(status, "active" | "done" | "failed" | "cancelled") {
        return "Error: status must be one of: active, done, failed, cancelled".to_string();
    }

    let summary = args["summary"].as_str();
    let artifact_ids = args["artifact_ids"]
        .as_array()
        .map(|v| serde_json::to_string(v).unwrap_or_default());

    match agents::update_status(&state.db, id, status, summary, artifact_ids.as_deref()).await {
        Ok(true) => format!("Agent {id} updated to '{status}'"),
        Ok(false) => format!("Agent {id} not found"),
        Err(e) => format!("Error: {e}"),
    }
}

pub async fn handle_list_agents(args: &Value, state: &Arc<AppState>) -> String {
    let domain = args["domain"].as_str().unwrap_or("");
    if domain.is_empty() {
        return "Error: domain is required".to_string();
    }

    let all = args["all"].as_bool().unwrap_or(false);

    let result = if all {
        agents::list_recent(&state.db, domain).await
    } else {
        agents::list_active(&state.db, domain).await
    };

    match result {
        Ok(list) if list.is_empty() => {
            if all {
                "No agents recorded for this domain.".to_string()
            } else {
                "No active agents. Use list_agents(all: true) to see completed agents.".to_string()
            }
        }
        Ok(list) => {
            let lines: Vec<String> = list
                .iter()
                .map(|a| {
                    let ended = a
                        .ended_at
                        .as_deref()
                        .map(|t| format!(", ended: {t}"))
                        .unwrap_or_default();
                    let summary = a
                        .summary
                        .as_deref()
                        .map(|s| format!("\n    summary: {s}"))
                        .unwrap_or_default();
                    format!(
                        "[{}] {} — {} ({}{}){}",
                        &a.id[..8],
                        a.role,
                        a.status,
                        a.spawned_at,
                        ended,
                        summary
                    )
                })
                .collect();
            format!("Agents for '{}':\n{}", domain, lines.join("\n"))
        }
        Err(e) => format!("Error: {e}"),
    }
}
