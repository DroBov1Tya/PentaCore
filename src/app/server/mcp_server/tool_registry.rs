use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use anyhow::Result;
use serde_json::Value;

use super::handlers;
use crate::app::AppState;

type ToolFuture = Pin<Box<dyn Future<Output = Result<String>> + Send>>;
type ToolFn = Arc<dyn Fn(Value, Arc<AppState>) -> ToolFuture + Send + Sync>;

pub struct ToolRegistry {
    tools: HashMap<&'static str, ToolFn>,
}

/// Wraps a `handler(&Value, &Arc<AppState>) -> String` into the owned
/// `(Value, Arc<AppState>) -> Result<String>` signature required by ToolFn.
macro_rules! h {
    ($handler:path) => {
        |args: Value, state: Arc<AppState>| async move { Ok($handler(&args, &state).await) }
    };
}

impl ToolRegistry {
    fn new() -> Self {
        Self {
            tools: HashMap::new(),
        }
    }

    fn register<F, Fut>(&mut self, name: &'static str, handler: F)
    where
        F: Fn(Value, Arc<AppState>) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<String>> + Send + 'static,
    {
        self.tools.insert(
            name,
            Arc::new(move |args, state| Box::pin(handler(args, state))),
        );
    }

    pub async fn dispatch(&self, name: &str, args: Value, state: Arc<AppState>) -> Result<String> {
        let Some(handler) = self.tools.get(name) else {
            return Err(anyhow::anyhow!("Unknown tool: {name}"));
        };

        let start = std::time::Instant::now();
        let result = handler(args, state).await;
        tracing::debug!(
            tool = name,
            elapsed_ms = start.elapsed().as_millis(),
            "tool executed"
        );

        result
    }

    pub fn build() -> Self {
        let mut r = Self::new();

        // Scope
        r.register("get_scope", h!(handlers::scope::handle_get_scope));
        r.register("save_scope", h!(handlers::scope::handle_save_scope));

        // Methodology FSM + Working Memory
        r.register(
            "get_phase_playbook",
            h!(handlers::methodology::handle_get_phase_playbook),
        );
        r.register(
            "transition_phase",
            h!(handlers::methodology::handle_transition_phase),
        );
        r.register(
            "save_hypothesis",
            h!(handlers::methodology::handle_save_hypothesis),
        );
        r.register(
            "get_hypotheses",
            h!(handlers::methodology::handle_get_hypotheses),
        );
        r.register(
            "update_hypothesis",
            h!(handlers::methodology::handle_update_hypothesis),
        );
        r.register(
            "save_dead_end",
            h!(handlers::methodology::handle_save_dead_end),
        );
        r.register(
            "recall_engagement_state",
            h!(handlers::methodology::handle_recall_engagement_state),
        );
        r.register(
            "record_lesson",
            h!(handlers::methodology::handle_record_lesson),
        );
        r.register(
            "recall_similar_situations",
            h!(handlers::methodology::handle_recall_similar_situations),
        );

        // Recon
        r.register("get_relations", h!(handlers::recon::handle_get_relations));
        r.register("save_relation", h!(handlers::recon::handle_save_relation));
        r.register("get_endpoints", h!(handlers::recon::handle_get_endpoints));
        r.register("save_endpoint", h!(handlers::recon::handle_save_endpoint));
        r.register(
            "enumerate_subdomains",
            h!(handlers::recon::handle_enumerate_subdomains),
        );
        r.register("resolve_dns", h!(handlers::recon::handle_resolve_dns));

        // RAG Memory
        r.register(
            "memorize_concept",
            h!(handlers::rag::handle_memorize_concept),
        );
        r.register(
            "search_knowledge",
            h!(handlers::rag::handle_search_knowledge),
        );
        r.register("list_memories", h!(handlers::rag::handle_list_memories));
        r.register("forget_memory", h!(handlers::rag::handle_forget_memory));
        r.register("get_memory", h!(handlers::rag::handle_get_memory));
        r.register("update_memory", h!(handlers::rag::handle_update_memory));

        // Findings + Credentials + Chains
        r.register("get_findings", h!(handlers::vulns::handle_get_findings));
        r.register("save_finding", h!(handlers::vulns::handle_save_finding));
        r.register("update_finding", h!(handlers::vulns::handle_update_finding));
        r.register(
            "get_credentials",
            h!(handlers::vulns::handle_get_credentials),
        );
        r.register(
            "save_credential",
            h!(handlers::vulns::handle_save_credential),
        );
        r.register("get_chains", h!(handlers::vulns::handle_get_chains));
        r.register("save_chain", h!(handlers::vulns::handle_save_chain));
        r.register(
            "get_chain_steps",
            h!(handlers::vulns::handle_get_chain_steps),
        );
        r.register("add_chain_step", h!(handlers::vulns::handle_add_chain_step));

        // Proxies
        r.register("get_proxies", h!(handlers::proxies::handle_get_proxies));
        r.register("save_proxy", h!(handlers::proxies::handle_save_proxy));

        // HTTP Client
        r.register("set_session", h!(handlers::client::handle_set_session));
        r.register(
            "revoke_session",
            h!(handlers::client::handle_revoke_session),
        );
        r.register("make_request", h!(handlers::client::handle_make_request));
        r.register(
            "make_race_requests",
            h!(handlers::client::handle_make_race_requests),
        );
        r.register("replay_as", h!(handlers::client::handle_replay_as));

        // Coverage + Requests
        r.register(
            "get_coverage",
            h!(handlers::coverage_requests::handle_get_coverage),
        );
        r.register(
            "upsert_coverage",
            h!(handlers::coverage_requests::handle_upsert_coverage),
        );
        r.register(
            "bulk_upsert_coverage",
            h!(handlers::coverage_requests::handle_bulk_upsert_coverage),
        );
        r.register(
            "get_requests",
            h!(handlers::coverage_requests::handle_get_requests),
        );
        r.register(
            "save_request",
            h!(handlers::coverage_requests::handle_save_request),
        );
        r.register(
            "bulk_save_requests",
            h!(handlers::coverage_requests::handle_bulk_save_requests),
        );
        r.register(
            "diff_requests",
            h!(handlers::coverage_requests::handle_diff_requests),
        );

        // Test Objects
        r.register(
            "claim_test_object",
            h!(handlers::test_objects::handle_claim_test_object),
        );
        r.register(
            "rollback_test_object",
            h!(handlers::test_objects::handle_rollback_test_object),
        );
        r.register(
            "get_test_objects",
            h!(handlers::test_objects::handle_get_test_objects),
        );

        // Parsers
        r.register(
            "save_endpoint_example",
            h!(handlers::parsers::handle_save_endpoint_example),
        );
        r.register(
            "get_endpoint_example",
            h!(handlers::parsers::handle_get_endpoint_example),
        );
        r.register(
            "parse_api_spec",
            h!(handlers::parsers::handle_parse_api_spec),
        );
        r.register(
            "parse_graphql_spec",
            h!(handlers::parsers::handle_parse_graphql_spec),
        );

        // Reporting
        r.register(
            "generate_report",
            h!(handlers::audit::handle_generate_report),
        );

        r
    }
}
