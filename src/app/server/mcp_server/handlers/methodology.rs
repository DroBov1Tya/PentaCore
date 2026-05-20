use crate::app::AppState;
use crate::app::database::queries::{engagement_state, methodology, playbook};
use serde_json::Value;
use std::sync::Arc;

pub async fn handle_get_phase_playbook(args: &Value, state: &Arc<AppState>) -> String {
    let domain = args["domain"].as_str().unwrap_or("");
    if domain.is_empty() {
        return "Error: domain is required".to_string();
    }

    match playbook::get_playbook(&state.db, domain).await {
        Ok(mut pb) => {
            let situation = format!(
                "{} phase pentest on {} — {} open hypotheses, {} dead ends",
                pb.current_phase, domain, pb.open_hypotheses_count, pb.dead_ends_count
            );
            let mut store = state.memory_store.lock().await;
            if let Ok(lessons) = store
                .search_by_category(&situation, "lesson", None, 3)
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
        Ok(id) => format!("Hypothesis saved. ID: {}", id),
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
    let reason = args["reason"].as_str().unwrap_or("");
    if domain.is_empty() || technique.is_empty() || reason.is_empty() {
        return "Error: domain, technique, and reason are required".to_string();
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
    let tags = args["tags"]
        .as_array()
        .map(|v| {
            v.iter()
                .filter_map(|s| s.as_str().map(String::from))
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let title = format!("{} → {}", trigger_pattern, outcome);
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
    if situation.is_empty() {
        return "Error: situation is required".to_string();
    }
    let mut store = state.memory_store.lock().await;
    match store
        .search_by_category(situation, "lesson", None, limit)
        .await
    {
        Ok(res) => serde_json::to_string_pretty(&res).unwrap_or_default(),
        Err(e) => format!("Error: {}", e),
    }
}
