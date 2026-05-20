use axum::{
    Router,
    routing::{delete, get, post, put},
};
use std::sync::Arc;

use super::handlers::{
    api_parser, attack_chains, client, coverage, credentials, dns, endpoint_examples, endpoints,
    findings, init, rag, requests, scope, summary, target_relations, test_objects,
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
        .route("/targets/{domain}/rag/memorize", post(rag::memorize))
        .route("/rag/search", get(rag::search))
        .route("/rag/memories", get(rag::list_memories))
        .route(
            "/rag/memories/{id}",
            get(rag::get_memory)
                .put(rag::update_memory)
                .delete(rag::forget),
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
        .route(
            "/targets/{domain}/test_objects",
            get(test_objects::list).post(test_objects::claim),
        )
        .route("/test_objects/{id}/rollback", post(test_objects::rollback))
        .route(
            "/endpoints/{endpoint_id}/examples",
            get(endpoint_examples::get).post(endpoint_examples::upsert),
        )
        .route(
            "/targets/{domain}/parse_openapi",
            post(api_parser::parse_openapi),
        )
        .route(
            "/targets/{domain}/parse_graphql",
            post(api_parser::parse_graphql),
        )
        .route("/dns/{domain}/resolve", get(dns::resolve))
        .route("/dns/{domain}/enumerate", get(dns::enumerate))
        .route("/bulk/coverage", post(coverage::bulk_upsert))
        .route("/bulk/requests", post(requests::bulk_save))
        .route("/requests/diff", get(requests::diff))
        .with_state(state)
}
