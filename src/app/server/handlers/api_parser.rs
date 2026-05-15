use crate::app::AppState;
use crate::app::client::api_parser;
use axum::{
    Json,
    extract::{Path, State},
};
use serde::Deserialize;
use std::sync::Arc;

#[derive(Deserialize)]
pub struct ParseSpecReq {
    pub url: Option<String>,
    pub json: Option<String>,
    pub base_endpoint: Option<String>,
}

pub async fn parse_openapi(
    State(state): State<Arc<AppState>>,
    Path(domain): Path<String>,
    Json(payload): Json<ParseSpecReq>,
) -> Result<Json<String>, String> {
    if payload.url.is_none() && payload.json.is_none() {
        return Err("Either url or json must be provided".to_string());
    }

    let json_content = if let Some(url) = &payload.url {
        let prereq = crate::app::client::req::PreRequest {
            cookie: vec![],
            method: "GET".to_string(),
            url: url.clone(),
            body: "".to_string(),
            proxy: None,
            user_agent: None,
        };
        match crate::app::client::req::make_req(prereq).await {
            Ok(resp) => match resp.text().await {
                Ok(text) => text,
                Err(e) => return Err(format!("Error reading response: {}", e)),
            },
            Err(e) => return Err(format!("Error downloading spec: {}", e)),
        }
    } else {
        payload.json.unwrap()
    };

    match api_parser::parse_and_import_openapi(&state.db, &domain, &json_content).await {
        Ok(summary) => Ok(Json(summary)),
        Err(e) => Err(e.to_string()),
    }
}

pub async fn parse_graphql(
    State(state): State<Arc<AppState>>,
    Path(domain): Path<String>,
    Json(payload): Json<ParseSpecReq>,
) -> Result<Json<String>, String> {
    if payload.url.is_none() && payload.json.is_none() {
        return Err("Either url or json must be provided".to_string());
    }

    let base_endpoint = payload
        .base_endpoint
        .unwrap_or_else(|| "/graphql".to_string());

    let json_content = if let Some(url) = &payload.url {
        let prereq = crate::app::client::req::PreRequest {
            cookie: vec![],
            method: "GET".to_string(),
            url: url.clone(),
            body: "".to_string(),
            proxy: None,
            user_agent: None,
        };
        match crate::app::client::req::make_req(prereq).await {
            Ok(resp) => match resp.text().await {
                Ok(text) => text,
                Err(e) => return Err(format!("Error reading response: {}", e)),
            },
            Err(e) => return Err(format!("Error downloading spec: {}", e)),
        }
    } else {
        payload.json.unwrap()
    };

    match api_parser::parse_and_import_graphql(&state.db, &domain, &json_content, &base_endpoint)
        .await
    {
        Ok(summary) => Ok(Json(summary)),
        Err(e) => Err(e.to_string()),
    }
}
