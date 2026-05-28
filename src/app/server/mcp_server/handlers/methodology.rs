use crate::app::AppState;
use crate::app::database::queries::{engagement_state, methodology, playbook};
use serde_json::Value;
use std::sync::Arc;

pub async fn handle_get_phase_playbook(args: &Value, state: &Arc<AppState>) -> String {
    let domain = args["domain"].as_str().unwrap_or("");
    if domain.is_empty() {
        return "Error: domain is required".to_string();
    }

    // Fetch domain_type from scope so recalled lessons only surface from matching engagements
    let scope_domain_type: Option<String> = sqlx::query_scalar(
        "SELECT s.domain_type FROM scopes s JOIN targets t ON t.id = s.target_id WHERE t.domain = ? ORDER BY s.created_at DESC LIMIT 1"
    )
    .bind(domain)
    .fetch_optional(&state.db)
    .await
    .unwrap_or(None)
    .flatten();

    match playbook::get_playbook(&state.db, domain).await {
        Ok(mut pb) => {
            let has_context = pb.open_hypotheses_count > 0 || pb.dead_ends_count > 0;
            if has_context {
                let situation = format!(
                    "{} phase pentest on {}, {} open hypotheses, {} dead ends",
                    pb.current_phase, domain, pb.open_hypotheses_count, pb.dead_ends_count
                );
                let tag_str: Option<String> = scope_domain_type
                    .as_ref()
                    .map(|dt| format!("domain_type:{}", dt));
                let tag_filter: Option<&str> = tag_str.as_deref();
                let mut store = state.memory_store.lock().await;
                if let Ok(lessons) = store
                    .search_by_category_tagged(&situation, "lesson", None, tag_filter, 3)
                    .await
                {
                    pb.recalled_lessons = lessons
                        .results
                        .into_iter()
                        .map(|note| {
                            serde_json::json!({
                                "title": note.title,
                                "content": note.content,
                                "similarity": note.score,
                                "warning": "Auto-recalled from past experience. Verify applicability."
                            })
                        })
                        .collect();
                }
            }
            serde_json::to_string_pretty(&pb).unwrap_or_default()
        }
        Err(e) => format!("Error: {}", e),
    }
}

pub async fn handle_transition_phase(args: &Value, state: &Arc<AppState>) -> String {
    let domain = args["domain"].as_str().unwrap_or("");
    let to_phase = args["to_phase"].as_str().unwrap_or("");
    let reason = args["reason"].as_str();
    if domain.is_empty() || to_phase.is_empty() {
        return "Error: domain and to_phase are required".to_string();
    }
    match playbook::transition(&state.db, domain, to_phase, reason).await {
        Ok(result) => serde_json::to_string_pretty(&result).unwrap_or_default(),
        Err(e) => format!("Error: {}", e),
    }
}

pub async fn handle_save_hypothesis(args: &Value, state: &Arc<AppState>) -> String {
    let domain = args["domain"].as_str().unwrap_or("");
    let hypothesis = args["hypothesis"].as_str().unwrap_or("");
    let source = args["source"].as_str();
    if domain.is_empty() || hypothesis.is_empty() {
        return "Error: domain and hypothesis are required".to_string();
    }
    let target_id: Option<i64> = sqlx::query_scalar("SELECT id FROM targets WHERE domain = ?")
        .bind(domain)
        .fetch_optional(&state.db)
        .await
        .unwrap_or(None);
    let target_id = match target_id {
        Some(id) => id,
        None => {
            return format!(
                "Error: target '{}' not found. Use save_scope first.",
                domain
            );
        }
    };
    match methodology::save_hypothesis(&state.db, target_id, hypothesis, source).await {
        Ok(id) => {
            // Nudge toward specificity without blocking. A vague hypothesis is saved
            // but the agent gets a hint so future agents have something actionable to work with.
            let hint = if hypothesis.len() < 60
                || !hypothesis.chars().any(|c| c == '/' || c == '.' || c == '(')
            {
                "\nHint: consider adding the specific endpoint, what you observed, and what a vulnerable response would look like."
            } else {
                ""
            };
            format!("Hypothesis saved. ID: {}{}", id, hint)
        }
        Err(e) => format!("Error: {}", e),
    }
}

pub async fn handle_get_hypotheses(args: &Value, state: &Arc<AppState>) -> String {
    let domain = args["domain"].as_str().unwrap_or("");
    let status = args["status"].as_str();
    if domain.is_empty() {
        return "Error: domain is required".to_string();
    }
    let target_id: Option<i64> = sqlx::query_scalar("SELECT id FROM targets WHERE domain = ?")
        .bind(domain)
        .fetch_optional(&state.db)
        .await
        .unwrap_or(None);
    let target_id = match target_id {
        Some(id) => id,
        None => return format!("Error: target '{}' not found", domain),
    };
    match methodology::get_hypotheses(&state.db, target_id, status).await {
        Ok(hyps) => serde_json::to_string_pretty(&hyps).unwrap_or_default(),
        Err(e) => format!("Error: {}", e),
    }
}

pub async fn handle_update_hypothesis(args: &Value, state: &Arc<AppState>) -> String {
    let id = args["id"].as_i64().unwrap_or(0);
    let status = args["status"].as_str();
    let evidence = args["evidence"].as_str();
    if id == 0 {
        return "Error: id is required".to_string();
    }
    match methodology::update_hypothesis(&state.db, id, status, evidence).await {
        Ok(_) => format!("Hypothesis {} updated.", id),
        Err(e) => format!("Error: {}", e),
    }
}

pub async fn handle_save_dead_end(args: &Value, state: &Arc<AppState>) -> String {
    let domain = args["domain"].as_str().unwrap_or("");
    let technique = args["technique"].as_str().unwrap_or("");
    let target_info = args["target_info"].as_str();
    let assumption = args["assumption_tested"].as_str().unwrap_or("");
    let expected = args["expected_if_vulnerable"].as_str().unwrap_or("");
    let base_reason = args["reason"].as_str().unwrap_or("");
    if domain.is_empty() || technique.is_empty() || base_reason.is_empty() {
        return "Error: domain, technique, and reason are required".to_string();
    }
    // Enrich the reason with assumption context so future agents see the full picture
    let reason_owned;
    let reason = if !assumption.is_empty() || !expected.is_empty() {
        reason_owned = format!(
            "{}\nASSUMPTION TESTED: {}\nEXPECTED IF VULNERABLE: {}",
            base_reason,
            if assumption.is_empty() {
                "not specified"
            } else {
                assumption
            },
            if expected.is_empty() {
                "not specified"
            } else {
                expected
            }
        );
        reason_owned.as_str()
    } else {
        base_reason
    };
    let target_id: Option<i64> = sqlx::query_scalar("SELECT id FROM targets WHERE domain = ?")
        .bind(domain)
        .fetch_optional(&state.db)
        .await
        .unwrap_or(None);
    let target_id = match target_id {
        Some(id) => id,
        None => return format!("Error: target '{}' not found", domain),
    };
    match methodology::save_dead_end(&state.db, target_id, technique, target_info, reason).await {
        Ok(id) => format!("Dead end recorded. ID: {}", id),
        Err(e) => format!("Error: {}", e),
    }
}

pub async fn handle_recall_engagement_state(args: &Value, state: &Arc<AppState>) -> String {
    let domain = args["domain"].as_str().unwrap_or("");
    let lens = args["lens"].as_str().unwrap_or("progress");
    if domain.is_empty() {
        return "Error: domain is required".to_string();
    }
    match engagement_state::recall(&state.db, domain, lens).await {
        Ok(result) => serde_json::to_string_pretty(&result).unwrap_or_default(),
        Err(e) => format!("Error: {}", e),
    }
}

pub async fn handle_record_lesson(args: &Value, state: &Arc<AppState>) -> String {
    let domain = args["domain"].as_str().unwrap_or("");
    let trigger_pattern = args["trigger_pattern"].as_str().unwrap_or("");
    let hypothesis = args["hypothesis"].as_str().unwrap_or("");
    let action_taken = args["action_taken"].as_str().unwrap_or("");
    let outcome = args["outcome"].as_str().unwrap_or("");
    let lesson = args["lesson"].as_str().unwrap_or("");
    if domain.is_empty() || trigger_pattern.is_empty() || lesson.is_empty() {
        return "Error: domain, trigger_pattern, and lesson are required".to_string();
    }
    let mut tags = args["tags"]
        .as_array()
        .map(|v| {
            v.iter()
                .filter_map(|s| s.as_str().map(String::from))
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    // Store domain_type as a tag so recall_similar_situations can filter cross-domain noise
    if let Some(dt) = args["domain_type"].as_str() {
        let valid = ["web", "binary", "cloud", "infra", "mobile"];
        if valid.contains(&dt) {
            tags.push(format!("domain_type:{}", dt));
        }
    }
    let title = format!("{} -> {}", trigger_pattern, outcome);
    let content = format!(
        "TRIGGER: {}\nHYPOTHESIS: {}\nACTION: {}\nOUTCOME: {}\nLESSON: {}",
        trigger_pattern, hypothesis, action_taken, outcome, lesson
    );
    let mut store = state.memory_store.lock().await;
    match store
        .memorize(domain, "lesson", &title, &content, &tags)
        .await
    {
        Ok(id) => format!("Lesson recorded. Memory ID: {}", id),
        Err(e) => format!("Error recording lesson: {}", e),
    }
}

pub async fn handle_recall_similar_situations(args: &Value, state: &Arc<AppState>) -> String {
    let situation = args["situation"].as_str().unwrap_or("");
    let limit = args["limit"].as_u64().unwrap_or(5) as usize;
    let domain_type = args["domain_type"].as_str();
    if situation.is_empty() {
        return "Error: situation is required".to_string();
    }
    let mut store = state.memory_store.lock().await;
    // If domain_type is provided, filter to matching lessons to avoid cross-domain noise
    let tag_str: Option<String> = domain_type.map(|dt| format!("domain_type:{}", dt));
    let tag_filter: Option<&str> = tag_str.as_deref();
    match store
        .search_by_category_tagged(situation, "lesson", None, tag_filter, limit)
        .await
    {
        Ok(res) => serde_json::to_string_pretty(&res).unwrap_or_default(),
        Err(e) => format!("Error: {}", e),
    }
}
