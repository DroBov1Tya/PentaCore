use crate::app::AppState;
use serde_json::Value;
use std::sync::Arc;

pub async fn handle_memorize_concept(args: &Value, state: &Arc<AppState>) -> String {
    let domain = args["domain"].as_str().unwrap_or("");
    let category = args["category"].as_str().unwrap_or("note");
    let title = args["title"].as_str().unwrap_or("");
    let content = args["content"].as_str().unwrap_or("");
    let tags = args["tags"]
        .as_array()
        .map(|v| {
            v.iter()
                .filter_map(|s| s.as_str().map(String::from))
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();

    let mut store = state.memory_store.lock().await;
    match store
        .memorize(domain, category, title, content, &tags)
        .await
    {
        Ok(id) => format!("Concept memorized successfully. Memory ID: {}", id),
        Err(e) => format!("Error memorizing concept: {}", e),
    }
}

pub async fn handle_search_knowledge(args: &Value, state: &Arc<AppState>) -> String {
    let query = args["query"].as_str().unwrap_or("");
    let limit = args["limit"].as_u64().unwrap_or(5) as usize;
    let target_domain = args["domain"].as_str();

    let mut store = state.memory_store.lock().await;
    match store.search(query, target_domain, limit).await {
        Ok(res) => serde_json::to_string_pretty(&res).unwrap_or_default(),
        Err(e) => format!("Error searching knowledge: {}", e),
    }
}

pub async fn handle_list_memories(args: &Value, state: &Arc<AppState>) -> String {
    let limit = args["limit"].as_u64().unwrap_or(10) as usize;
    let target_domain = args["domain"].as_str();

    let store = state.memory_store.lock().await;
    match store.list_memories(target_domain, limit).await {
        Ok(res) => serde_json::to_string_pretty(&res).unwrap_or_default(),
        Err(e) => format!("Error listing memories: {}", e),
    }
}

pub async fn handle_forget_memory(args: &Value, state: &Arc<AppState>) -> String {
    let id = args["id"].as_str().unwrap_or("");
    if id.is_empty() {
        return "Error: id is required".to_string();
    }

    let store = state.memory_store.lock().await;
    match store.forget(id).await {
        Ok(_) => format!("Memory {} deleted successfully.", id),
        Err(e) => format!("Error deleting memory: {}", e),
    }
}

pub async fn handle_get_memory(args: &Value, state: &Arc<AppState>) -> String {
    let id = args["id"].as_str().unwrap_or("");
    if id.is_empty() {
        return "Error: id is required".to_string();
    }

    let store = state.memory_store.lock().await;
    match store.get_memory(id).await {
        Ok(Some(mem)) => serde_json::to_string_pretty(&mem).unwrap_or_default(),
        Ok(None) => format!("Memory with id {} not found", id),
        Err(e) => format!("Error retrieving memory: {}", e),
    }
}

pub async fn handle_update_memory(args: &Value, state: &Arc<AppState>) -> String {
    let id = args["id"].as_str().unwrap_or("");
    if id.is_empty() {
        return "Error: id is required".to_string();
    }

    let category = args["category"].as_str();
    let title = args["title"].as_str();
    let content = args["content"].as_str();

    let tags_vec: Option<Vec<String>> = args["tags"].as_array().map(|v| {
        v.iter()
            .filter_map(|s| s.as_str().map(String::from))
            .collect()
    });

    let mut store = state.memory_store.lock().await;
    match store
        .update_memory(id, category, title, content, tags_vec.as_deref())
        .await
    {
        Ok(_) => format!("Memory {} updated successfully.", id),
        Err(e) => format!("Error updating memory: {}", e),
    }
}
