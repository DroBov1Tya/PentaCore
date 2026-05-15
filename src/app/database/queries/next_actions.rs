use serde::Serialize;
use sqlx::SqlitePool;

#[derive(Serialize)]
pub struct NextActions {
    pub domain: String,
    pub phase: String,
    pub stats: AuditStats,
    pub actions: Vec<Action>,
}

#[derive(Serialize)]
pub struct AuditStats {
    pub total_endpoints: i64,
    pub untested_endpoints: i64,
    pub total_findings: i64,
    pub confirmed_findings: i64,
    pub potential_findings: i64,
    pub coverage_pending: i64,
    pub coverage_done: i64,
    pub coverage_skipped: i64,
    pub active_test_objects: i64,
    pub total_subdomains: i64,
    pub total_credentials: i64,
    pub total_requests: i64,
}

#[derive(Serialize)]
pub struct Action {
    pub priority: u8,
    pub category: String,
    pub action: String,
    pub reason: String,
}

pub async fn get(db: &SqlitePool, domain: &str) -> sqlx::Result<NextActions> {
    let target_id: Option<i64> = sqlx::query_scalar("SELECT id FROM targets WHERE domain = ?")
        .bind(domain)
        .fetch_optional(db)
        .await?;

    let target_id = match target_id {
        Some(id) => id,
        None => {
            return Ok(NextActions {
                domain: domain.to_string(),
                phase: "not_started".to_string(),
                stats: AuditStats {
                    total_endpoints: 0,
                    untested_endpoints: 0,
                    total_findings: 0,
                    confirmed_findings: 0,
                    potential_findings: 0,
                    coverage_pending: 0,
                    coverage_done: 0,
                    coverage_skipped: 0,
                    active_test_objects: 0,
                    total_subdomains: 0,
                    total_credentials: 0,
                    total_requests: 0,
                },
                actions: vec![Action {
                    priority: 1,
                    category: "setup".to_string(),
                    action: format!("Run save_scope for {} to define engagement rules", domain),
                    reason: "Target not registered yet".to_string(),
                }],
            });
        }
    };

    let total_endpoints: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM endpoints WHERE target_id = ?")
            .bind(target_id)
            .fetch_one(db)
            .await?;

    let untested_endpoints: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM endpoints e WHERE e.target_id = ? AND NOT EXISTS (SELECT 1 FROM coverage c WHERE c.endpoint_id = e.id)"
    ).bind(target_id).fetch_one(db).await?;

    let total_findings: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM findings WHERE target_id = ?")
            .bind(target_id)
            .fetch_one(db)
            .await?;

    let confirmed_findings: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM findings WHERE target_id = ? AND status = 'confirmed'",
    )
    .bind(target_id)
    .fetch_one(db)
    .await?;

    let potential_findings: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM findings WHERE target_id = ? AND status = 'potential'",
    )
    .bind(target_id)
    .fetch_one(db)
    .await?;

    let coverage_pending: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM coverage c JOIN endpoints e ON e.id = c.endpoint_id WHERE e.target_id = ? AND c.status = 'pending'"
    ).bind(target_id).fetch_one(db).await?;

    let coverage_done: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM coverage c JOIN endpoints e ON e.id = c.endpoint_id WHERE e.target_id = ? AND c.status = 'done'"
    ).bind(target_id).fetch_one(db).await?;

    let coverage_skipped: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM coverage c JOIN endpoints e ON e.id = c.endpoint_id WHERE e.target_id = ? AND c.status = 'skipped'"
    ).bind(target_id).fetch_one(db).await?;

    let active_test_objects: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM test_objects WHERE target_id = ? AND status = 'active'",
    )
    .bind(target_id)
    .fetch_one(db)
    .await?;

    let total_subdomains: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM target_relations WHERE from_id = ? AND type = 'subdomain'",
    )
    .bind(target_id)
    .fetch_one(db)
    .await?;

    let total_credentials: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM credentials WHERE target_id = ?")
            .bind(target_id)
            .fetch_one(db)
            .await?;

    let total_requests: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM requests r JOIN endpoints e ON e.id = r.endpoint_id WHERE e.target_id = ?"
    ).bind(target_id).fetch_one(db).await?;

    let stats = AuditStats {
        total_endpoints,
        untested_endpoints,
        total_findings,
        confirmed_findings,
        potential_findings,
        coverage_pending,
        coverage_done,
        coverage_skipped,
        active_test_objects,
        total_subdomains,
        total_credentials,
        total_requests,
    };

    let has_scope: bool =
        sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM scopes WHERE target_id = ?")
            .bind(target_id)
            .fetch_one(db)
            .await?
            > 0;

    let phase = if !has_scope {
        "setup"
    } else if total_endpoints == 0 {
        "recon"
    } else if untested_endpoints > total_endpoints / 2 {
        "early_testing"
    } else if potential_findings > 0 && confirmed_findings == 0 {
        "validation"
    } else if coverage_pending > 0 {
        "deep_testing"
    } else {
        "reporting"
    };

    let mut actions: Vec<Action> = Vec::new();

    if !has_scope {
        actions.push(Action {
            priority: 1,
            category: "setup".into(),
            action: "Define scope with save_scope before any testing".into(),
            reason:
                "No scope/rules defined — testing without authorization boundaries is dangerous"
                    .into(),
        });
    }

    if total_endpoints == 0 && has_scope {
        actions.push(Action {
            priority: 1,
            category: "recon".into(),
            action: "Run enumerate_subdomains and resolve_dns to discover infrastructure".into(),
            reason: "No endpoints discovered yet".into(),
        });
        actions.push(Action {
            priority: 2,
            category: "recon".into(),
            action: "Look for swagger.json/openapi.json and use parse_api_spec to import routes"
                .into(),
            reason: "API spec parsing is the fastest way to build endpoint inventory".into(),
        });
    }

    if total_subdomains == 0 && total_endpoints > 0 {
        actions.push(Action {
            priority: 2,
            category: "recon".into(),
            action: "Run enumerate_subdomains — no subdomain relations exist yet".into(),
            reason:
                "Hidden subdomains often expose admin panels, staging envs, or legacy endpoints"
                    .into(),
        });
    }

    if untested_endpoints > 0 {
        // Find auth endpoints without coverage
        let auth_untested: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM endpoints e WHERE e.target_id = ? AND e.auth = 1 AND NOT EXISTS (SELECT 1 FROM coverage c WHERE c.endpoint_id = e.id)"
        ).bind(target_id).fetch_one(db).await?;

        if auth_untested > 0 {
            actions.push(Action {
                priority: 1,
                category: "auth".into(),
                action: format!(
                    "{} auth-required endpoints have zero coverage — test IDOR/BOLA first",
                    auth_untested
                ),
                reason: "Auth endpoints are highest-value targets for access control bypasses"
                    .into(),
            });
        }

        actions.push(Action {
            priority: 2, category: "testing".into(),
            action: format!("{}/{} endpoints have no coverage records — create coverage entries with upsert_coverage", untested_endpoints, total_endpoints),
            reason: "Coverage tracking prevents duplicate work and ensures completeness".into(),
        });
    }

    if coverage_pending > 0 {
        let pending_vectors: Vec<String> = sqlx::query_scalar(
            "SELECT DISTINCT c.vector FROM coverage c JOIN endpoints e ON e.id = c.endpoint_id WHERE e.target_id = ? AND c.status = 'pending' LIMIT 5"
        ).bind(target_id).fetch_all(db).await?;

        actions.push(Action {
            priority: 2,
            category: "testing".into(),
            action: format!(
                "{} coverage entries pending: [{}]",
                coverage_pending,
                pending_vectors.join(", ")
            ),
            reason: "Pending vectors are registered but not yet tested".into(),
        });
    }

    if potential_findings > 0 {
        actions.push(Action {
            priority: 1, category: "validation".into(),
            action: format!("{} findings are 'potential' — validate with PoC and promote to 'confirmed' or 'false_positive'", potential_findings),
            reason: "Unvalidated findings cannot be reported. Each needs reproducible evidence with raw request/response".into(),
        });
    }

    if active_test_objects > 0 {
        actions.push(Action {
            priority: 1,
            category: "cleanup".into(),
            action: format!(
                "{} test objects are still 'active' — rollback before session ends",
                active_test_objects
            ),
            reason:
                "Leftover test artifacts violate responsible disclosure. Use rollback_test_object"
                    .into(),
        });
    }

    if confirmed_findings > 0 {
        let no_evidence: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM findings WHERE target_id = ? AND status = 'confirmed' AND (evidence IS NULL OR evidence = '')"
        ).bind(target_id).fetch_one(db).await?;

        if no_evidence > 0 {
            actions.push(Action {
                priority: 1,
                category: "reporting".into(),
                action: format!(
                    "{} confirmed findings lack evidence field — add raw proof before reporting",
                    no_evidence
                ),
                reason: "Bug bounty reports without evidence are rejected".into(),
            });
        }
    }

    if phase == "reporting" && confirmed_findings > 0 {
        actions.push(Action {
            priority: 2, category: "reporting".into(),
            action: "Generate final report: gather confirmed findings, attach evidence, verify reproducibility".into(),
            reason: "All coverage done, confirmed findings exist — ready for report compilation".into(),
        });
    }

    actions.sort_by_key(|a| a.priority);

    Ok(NextActions {
        domain: domain.to_string(),
        phase: phase.to_string(),
        stats,
        actions,
    })
}
