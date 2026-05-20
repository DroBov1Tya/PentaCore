use crate::app::client::dns;
use axum::{Json, extract::Path};

pub async fn resolve(Path(domain): Path<String>) -> Result<Json<Vec<String>>, String> {
    if domain.is_empty() {
        return Err("domain is required".to_string());
    }
    let results = dns::resolve_dns(&domain).await;
    Ok(Json(results))
}

pub async fn enumerate(
    axum::extract::State(state): axum::extract::State<std::sync::Arc<crate::app::AppState>>,
    Path(domain): Path<String>,
) -> Result<Json<Vec<String>>, String> {
    if domain.is_empty() {
        return Err("domain is required".to_string());
    }

    let results = dns::enumerate_subdomains(&domain).await;

    for res in &results {
        if let Some(sub) = res.split(" ->").next() {
            let rel = crate::app::database::queries::target_relations::CreateRelation {
                to_domain: sub.to_string(),
                rel_type: "subdomain".to_string(),
                description: Some("Discovered via enumerate_subdomains".to_string()),
            };
            let _ =
                crate::app::database::queries::target_relations::create(&state.db, &domain, &rel)
                    .await;
        }
    }

    Ok(Json(results))
}
