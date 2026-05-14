use axum::{
    Router,
    routing::{get, post},
};
use std::sync::Arc;

use super::handlers::{
    attack_chains, coverage, credentials, endpoints, findings, init, requests, summary,
};
use crate::app::AppState;


pub fn build_router(state: Arc<AppState>) -> Router {
    Router::new()
        .route("/init", get(init::instructions))
        // Target context
        .route("/targets/{domain}/summary", get(summary::get))
        .route(
            "/targets/{domain}/endpoints",
            get(endpoints::list).post(endpoints::create),
        )
        .route(
            "/targets/{domain}/findings",
            get(findings::list).post(findings::create),
        )
        .route(
            "/targets/{domain}/credentials",
            get(credentials::list).post(credentials::create),
        )
        // Attack chains
        .route(
            "/targets/{domain}/chains",
            get(attack_chains::list).post(attack_chains::create),
        )
        .route(
            "/chains/{chain_id}/steps",
            get(attack_chains::get_steps).post(attack_chains::add_step),
        )
        // Endpoint-level
        .route(
            "/endpoints/{endpoint_id}/requests",
            get(requests::list).post(requests::create),
        )
        .route(
            "/endpoints/{endpoint_id}/coverage",
            get(coverage::list).post(coverage::upsert),
        )
        .with_state(state)
}
