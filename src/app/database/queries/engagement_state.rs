use serde::Serialize;
use sqlx::{FromRow, SqlitePool};

use super::methodology;

/// Lens-based engagement state recall.
/// Each lens returns a different slice of the engagement — minimal data, maximum relevance.

#[derive(Serialize)]
pub struct EngagementState {
    pub domain: String,
    pub lens: String,
    pub data: serde_json::Value,
}

// Lens: progress

#[derive(Serialize)]
pub struct ProgressLens {
    pub current_phase: String,
    pub total_endpoints: i64,
    pub untested_endpoints: i64,
    pub total_findings: i64,
    pub confirmed_findings: i64,
    pub potential_findings: i64,
    pub coverage_done: i64,
    pub coverage_pending: i64,
    pub coverage_total: i64,
    pub coverage_percent: f64,
    pub open_hypotheses: i64,
    pub dead_ends: i64,
    pub total_credentials: i64,
    pub active_test_objects: i64,
    pub total_requests: i64,
}

// Lens: hosts

#[derive(Serialize, FromRow)]
pub struct HostInfo {
    pub domain: String,
    pub rel_type: String,
    pub description: Option<String>,
}

// Lens: creds

#[derive(Serialize, FromRow)]
pub struct CredInfo {
    pub id: i64,
    pub cred_type: String,
    pub username: Option<String>,
    pub secret: String,
    pub description: Option<String>,
}

// Lens: attack_surface

#[derive(Serialize, FromRow)]
pub struct AttackSurfaceEndpoint {
    pub endpoint_id: i64,
    pub method: String,
    pub path: String,
    pub auth: i32,
    pub pending_vectors: String,
}

// Main recall function

pub async fn recall(db: &SqlitePool, domain: &str, lens: &str) -> sqlx::Result<EngagementState> {
    let target_id: Option<i64> = sqlx::query_scalar("SELECT id FROM targets WHERE domain = ?")
        .bind(domain)
        .fetch_optional(db)
        .await?;

    let target_id = match target_id {
        Some(id) => id,
        None => {
            return Ok(EngagementState {
                domain: domain.to_string(),
                lens: lens.to_string(),
                data: serde_json::json!({ "error": "Target not found. Use save_scope to register." }),
            });
        }
    };

    let data = match lens {
        "progress" => recall_progress(db, target_id).await?,
        "hosts" => recall_hosts(db, target_id).await?,
        "creds" => recall_creds(db, target_id).await?,
        "open_hypotheses" => recall_open_hypotheses(db, target_id).await?,
        "dead_ends" => recall_dead_ends(db, target_id).await?,
        "attack_surface" => recall_attack_surface(db, target_id).await?,
        _ => serde_json::json!({
            "error": "Unknown lens. Valid: progress, hosts, creds, open_hypotheses, dead_ends, attack_surface"
        }),
    };

    Ok(EngagementState {
        domain: domain.to_string(),
        lens: lens.to_string(),
        data,
    })
}

async fn recall_progress(db: &SqlitePool, target_id: i64) -> sqlx::Result<serde_json::Value> {
    let current_phase = methodology::get_current_phase(db, target_id)
        .await?
        .unwrap_or_else(|| "setup".to_string());

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

    let coverage_done: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM coverage c JOIN endpoints e ON e.id = c.endpoint_id WHERE e.target_id = ? AND c.status = 'done'"
    ).bind(target_id).fetch_one(db).await?;
    let coverage_pending: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM coverage c JOIN endpoints e ON e.id = c.endpoint_id WHERE e.target_id = ? AND c.status = 'pending'"
    ).bind(target_id).fetch_one(db).await?;
    let coverage_total: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM coverage c JOIN endpoints e ON e.id = c.endpoint_id WHERE e.target_id = ?"
    ).bind(target_id).fetch_one(db).await?;

    let coverage_percent = if coverage_total > 0 {
        (coverage_done as f64 / coverage_total as f64) * 100.0
    } else {
        0.0
    };

    let open_hypotheses: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM hypotheses WHERE target_id = ? AND status IN ('open', 'testing')",
    )
    .bind(target_id)
    .fetch_one(db)
    .await?;
    let dead_ends: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM dead_ends WHERE target_id = ?")
        .bind(target_id)
        .fetch_one(db)
        .await?;
    let total_credentials: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM credentials WHERE target_id = ?")
            .bind(target_id)
            .fetch_one(db)
            .await?;
    let active_test_objects: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM test_objects WHERE target_id = ? AND status = 'active'",
    )
    .bind(target_id)
    .fetch_one(db)
    .await?;
    let total_requests: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM requests r JOIN endpoints e ON e.id = r.endpoint_id WHERE e.target_id = ?"
    ).bind(target_id).fetch_one(db).await?;

    let progress = ProgressLens {
        current_phase,
        total_endpoints,
        untested_endpoints,
        total_findings,
        confirmed_findings,
        potential_findings,
        coverage_done,
        coverage_pending,
        coverage_total,
        coverage_percent,
        open_hypotheses,
        dead_ends,
        total_credentials,
        active_test_objects,
        total_requests,
    };

    Ok(serde_json::to_value(progress).unwrap_or_default())
}

async fn recall_hosts(db: &SqlitePool, target_id: i64) -> sqlx::Result<serde_json::Value> {
    let hosts = sqlx::query_as::<_, HostInfo>(
        r#"SELECT t2.domain, tr.type as rel_type, tr.description
           FROM target_relations tr
           JOIN targets t2 ON t2.id = tr.to_id
           WHERE tr.from_id = ?
           ORDER BY tr.type, t2.domain"#,
    )
    .bind(target_id)
    .fetch_all(db)
    .await?;

    Ok(serde_json::to_value(hosts).unwrap_or_default())
}

async fn recall_creds(db: &SqlitePool, target_id: i64) -> sqlx::Result<serde_json::Value> {
    let creds = sqlx::query_as::<_, CredInfo>(
        r#"SELECT id, type as cred_type, username, secret, description
           FROM credentials WHERE target_id = ?
           ORDER BY created_at DESC"#,
    )
    .bind(target_id)
    .fetch_all(db)
    .await?;

    Ok(serde_json::to_value(creds).unwrap_or_default())
}

async fn recall_open_hypotheses(
    db: &SqlitePool,
    target_id: i64,
) -> sqlx::Result<serde_json::Value> {
    let hyps = methodology::get_hypotheses(db, target_id, None).await?;
    // Filter to open + testing only
    let open: Vec<_> = hyps
        .into_iter()
        .filter(|h| h.status == "open" || h.status == "testing")
        .collect();
    Ok(serde_json::to_value(open).unwrap_or_default())
}

async fn recall_dead_ends(db: &SqlitePool, target_id: i64) -> sqlx::Result<serde_json::Value> {
    let ends = methodology::get_dead_ends(db, target_id).await?;
    Ok(serde_json::to_value(ends).unwrap_or_default())
}

async fn recall_attack_surface(db: &SqlitePool, target_id: i64) -> sqlx::Result<serde_json::Value> {
    // Endpoints with their pending coverage vectors
    let rows = sqlx::query_as::<_, AttackSurfaceEndpoint>(
        r#"SELECT
               e.id as endpoint_id,
               e.method,
               e.path,
               e.auth,
               COALESCE(
                   (SELECT GROUP_CONCAT(c.vector, ', ')
                    FROM coverage c
                    WHERE c.endpoint_id = e.id AND c.status IN ('pending', 'in_progress')),
                   'NO_COVERAGE'
               ) as pending_vectors
           FROM endpoints e
           WHERE e.target_id = ?
             AND (
               NOT EXISTS (SELECT 1 FROM coverage c WHERE c.endpoint_id = e.id)
               OR EXISTS (SELECT 1 FROM coverage c WHERE c.endpoint_id = e.id AND c.status IN ('pending', 'in_progress'))
             )
           ORDER BY e.auth DESC, e.path"#,
    )
    .bind(target_id)
    .fetch_all(db)
    .await?;

    Ok(serde_json::to_value(rows).unwrap_or_default())
}
