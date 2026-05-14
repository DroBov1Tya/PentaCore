use axum::{
    Router,
    routing::{delete, get, post},
};
use std::sync::Arc;

use super::handlers::{
    attack_chains, client, coverage, credentials, endpoints, findings, init, requests, scope,
    summary, target_relations,
};
use crate::app::AppState;

pub fn build_router(state: Arc<AppState>) -> Router {
    Router::new()
        .route("/", get(init::instructions).post(init::initialize))
        .route("/targets/{domain}/summary", get(summary::get))
        .route(
            "/targets/{domain}/scope",
            get(scope::get).post(scope::upsert),
        )
        .route(
            "/targets/{domain}/relations",
            get(target_relations::list).post(target_relations::create),
        )
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
        .route(
            "/targets/{domain}/chains",
            get(attack_chains::list).post(attack_chains::create),
        )
        .route(
            "/chains/{chain_id}/steps",
            get(attack_chains::get_steps).post(attack_chains::add_step),
        )
        .route(
            "/endpoints/{endpoint_id}/requests",
            get(requests::list).post(requests::create),
        )
        .route(
            "/endpoints/{endpoint_id}/coverage",
            get(coverage::list).post(coverage::upsert),
        )
        .route(
            "/client/session",
            post(client::set_session).delete(client::revoke_session),
        )
        .route("/client/request", post(client::make_request_handler))
        .route("/client/race", post(client::make_race_requests_handler))
        .with_state(state)
}
