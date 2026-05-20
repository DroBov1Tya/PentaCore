use serde::Serialize;
use sqlx::{FromRow, SqlitePool};

// Hypotheses

#[derive(Serialize, FromRow)]
pub struct Hypothesis {
    pub id: i64,
    pub hypothesis: String,
    pub status: String,
    pub evidence: Option<String>,
    pub source: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

pub async fn save_hypothesis(
    db: &SqlitePool,
    target_id: i64,
    hypothesis: &str,
    source: Option<&str>,
) -> sqlx::Result<i64> {
    let id: i64 = sqlx::query_scalar(
        "INSERT INTO hypotheses (target_id, hypothesis, source) VALUES (?, ?, ?) RETURNING id",
    )
    .bind(target_id)
    .bind(hypothesis)
    .bind(source)
    .fetch_one(db)
    .await?;
    Ok(id)
}

pub async fn get_hypotheses(
    db: &SqlitePool,
    target_id: i64,
    status_filter: Option<&str>,
) -> sqlx::Result<Vec<Hypothesis>> {
    if let Some(status) = status_filter {
        sqlx::query_as::<_, Hypothesis>(
            "SELECT id, hypothesis, status, evidence, source, created_at, updated_at FROM hypotheses WHERE target_id = ? AND status = ? ORDER BY created_at DESC",
        )
        .bind(target_id)
        .bind(status)
        .fetch_all(db)
        .await
    } else {
        sqlx::query_as::<_, Hypothesis>(
            "SELECT id, hypothesis, status, evidence, source, created_at, updated_at FROM hypotheses WHERE target_id = ? ORDER BY created_at DESC",
        )
        .bind(target_id)
        .fetch_all(db)
        .await
    }
}

pub async fn update_hypothesis(
    db: &SqlitePool,
    id: i64,
    status: Option<&str>,
    evidence: Option<&str>,
) -> sqlx::Result<()> {
    if let Some(s) = status {
        sqlx::query(
            "UPDATE hypotheses SET status = ?, updated_at = CURRENT_TIMESTAMP WHERE id = ?",
        )
        .bind(s)
        .bind(id)
        .execute(db)
        .await?;
    }
    if let Some(e) = evidence {
        sqlx::query(
            "UPDATE hypotheses SET evidence = ?, updated_at = CURRENT_TIMESTAMP WHERE id = ?",
        )
        .bind(e)
        .bind(id)
        .execute(db)
        .await?;
    }
    Ok(())
}

// Dead Ends

#[derive(Serialize, FromRow)]
pub struct DeadEnd {
    pub id: i64,
    pub technique: String,
    pub target_info: Option<String>,
    pub reason: String,
    pub created_at: String,
}

pub async fn save_dead_end(
    db: &SqlitePool,
    target_id: i64,
    technique: &str,
    target_info: Option<&str>,
    reason: &str,
) -> sqlx::Result<i64> {
    let id: i64 = sqlx::query_scalar(
        "INSERT INTO dead_ends (target_id, technique, target_info, reason) VALUES (?, ?, ?, ?) RETURNING id",
    )
    .bind(target_id)
    .bind(technique)
    .bind(target_info)
    .bind(reason)
    .fetch_one(db)
    .await?;
    Ok(id)
}

pub async fn get_dead_ends(db: &SqlitePool, target_id: i64) -> sqlx::Result<Vec<DeadEnd>> {
    sqlx::query_as::<_, DeadEnd>(
        "SELECT id, technique, target_info, reason, created_at FROM dead_ends WHERE target_id = ? ORDER BY created_at DESC",
    )
    .bind(target_id)
    .fetch_all(db)
    .await
}

// Phase Transitions

#[derive(Serialize, FromRow)]
pub struct PhaseTransition {
    pub id: i64,
    pub from_phase: String,
    pub to_phase: String,
    pub reason: Option<String>,
    pub created_at: String,
}

pub async fn save_phase_transition(
    db: &SqlitePool,
    target_id: i64,
    from_phase: &str,
    to_phase: &str,
    reason: Option<&str>,
) -> sqlx::Result<i64> {
    let id: i64 = sqlx::query_scalar(
        "INSERT INTO phase_transitions (target_id, from_phase, to_phase, reason) VALUES (?, ?, ?, ?) RETURNING id",
    )
    .bind(target_id)
    .bind(from_phase)
    .bind(to_phase)
    .bind(reason)
    .fetch_one(db)
    .await?;
    Ok(id)
}

pub async fn get_current_phase(db: &SqlitePool, target_id: i64) -> sqlx::Result<Option<String>> {
    sqlx::query_scalar::<_, String>(
        "SELECT to_phase FROM phase_transitions WHERE target_id = ? ORDER BY created_at DESC LIMIT 1",
    )
    .bind(target_id)
    .fetch_optional(db)
    .await
}

pub async fn get_phase_history(
    db: &SqlitePool,
    target_id: i64,
) -> sqlx::Result<Vec<PhaseTransition>> {
    sqlx::query_as::<_, PhaseTransition>(
        "SELECT id, from_phase, to_phase, reason, created_at FROM phase_transitions WHERE target_id = ? ORDER BY created_at ASC",
    )
    .bind(target_id)
    .fetch_all(db)
    .await
}
