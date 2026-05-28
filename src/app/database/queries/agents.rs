use anyhow::Result;
use serde::{Deserialize, Serialize};
use sqlx::{Pool, Sqlite};
use uuid::Uuid;

#[derive(Debug, Serialize, Deserialize)]
pub struct Agent {
    pub id: String,
    pub domain: String,
    pub role: String,
    pub objective: String,
    pub status: String,
    pub spawned_at: String,
    pub ended_at: Option<String>,
    pub summary: Option<String>,
    pub artifact_ids: String,
}

pub async fn spawn(db: &Pool<Sqlite>, domain: &str, role: &str, objective: &str) -> Result<String> {
    sweep_stale(db, domain).await?;

    let id = Uuid::new_v4().to_string();
    sqlx::query("INSERT INTO agents (id, domain, role, objective) VALUES (?, ?, ?, ?)")
        .bind(&id)
        .bind(domain)
        .bind(role)
        .bind(objective)
        .execute(db)
        .await?;

    Ok(id)
}

pub async fn update_status(
    db: &Pool<Sqlite>,
    id: &str,
    status: &str,
    summary: Option<&str>,
    artifact_ids: Option<&str>,
) -> Result<bool> {
    let ended_at: Option<&str> = if matches!(status, "done" | "failed" | "cancelled") {
        Some("CURRENT_TIMESTAMP")
    } else {
        None
    };

    let rows = if ended_at.is_some() {
        sqlx::query(
            "UPDATE agents SET status = ?, summary = ?, artifact_ids = COALESCE(?, artifact_ids),
             ended_at = CURRENT_TIMESTAMP WHERE id = ?",
        )
        .bind(status)
        .bind(summary)
        .bind(artifact_ids)
        .bind(id)
        .execute(db)
        .await?
        .rows_affected()
    } else {
        sqlx::query(
            "UPDATE agents SET status = ?, summary = ?, artifact_ids = COALESCE(?, artifact_ids)
             WHERE id = ?",
        )
        .bind(status)
        .bind(summary)
        .bind(artifact_ids)
        .bind(id)
        .execute(db)
        .await?
        .rows_affected()
    };

    Ok(rows > 0)
}

pub async fn list_active(db: &Pool<Sqlite>, domain: &str) -> Result<Vec<Agent>> {
    let rows = sqlx::query_as::<
        _,
        (
            String,
            String,
            String,
            String,
            String,
            String,
            Option<String>,
            Option<String>,
            String,
        ),
    >(
        "SELECT id, domain, role, objective, status, spawned_at, ended_at, summary, artifact_ids
         FROM agents WHERE domain = ? AND status = 'active'
         ORDER BY spawned_at DESC",
    )
    .bind(domain)
    .fetch_all(db)
    .await?;

    Ok(rows
        .into_iter()
        .map(
            |(id, domain, role, objective, status, spawned_at, ended_at, summary, artifact_ids)| {
                Agent {
                    id,
                    domain,
                    role,
                    objective,
                    status,
                    spawned_at,
                    ended_at,
                    summary,
                    artifact_ids,
                }
            },
        )
        .collect())
}

pub async fn list_recent(db: &Pool<Sqlite>, domain: &str) -> Result<Vec<Agent>> {
    let rows = sqlx::query_as::<
        _,
        (
            String,
            String,
            String,
            String,
            String,
            String,
            Option<String>,
            Option<String>,
            String,
        ),
    >(
        "SELECT id, domain, role, objective, status, spawned_at, ended_at, summary, artifact_ids
         FROM agents WHERE domain = ?
         ORDER BY spawned_at DESC LIMIT 20",
    )
    .bind(domain)
    .fetch_all(db)
    .await?;

    Ok(rows
        .into_iter()
        .map(
            |(id, domain, role, objective, status, spawned_at, ended_at, summary, artifact_ids)| {
                Agent {
                    id,
                    domain,
                    role,
                    objective,
                    status,
                    spawned_at,
                    ended_at,
                    summary,
                    artifact_ids,
                }
            },
        )
        .collect())
}

/// Cleans up finished agents older than 1 hour for this domain,
/// and marks agents stuck in 'active' for more than 4 hours as 'failed'.
/// Called automatically on every spawn — no manual cleanup needed.
pub async fn sweep_stale(db: &Pool<Sqlite>, domain: &str) -> Result<()> {
    // Mark crashed agents (active > 4 hours) as failed
    sqlx::query(
        "UPDATE agents SET status = 'failed', ended_at = CURRENT_TIMESTAMP,
         summary = 'Auto-failed: no update in 4+ hours'
         WHERE domain = ? AND status = 'active'
         AND spawned_at < datetime('now', '-4 hours')",
    )
    .bind(domain)
    .execute(db)
    .await?;

    // Delete completed agents older than 1 hour
    sqlx::query(
        "DELETE FROM agents WHERE domain = ? AND status IN ('done','failed','cancelled')
         AND COALESCE(ended_at, spawned_at) < datetime('now', '-1 hour')",
    )
    .bind(domain)
    .execute(db)
    .await?;

    Ok(())
}
