use serde::{Deserialize, Serialize};
use sqlx::{FromRow, SqlitePool};

#[derive(Serialize, FromRow)]
pub struct ChainRow {
    pub id: i64,
    pub target_id: i64,
    pub title: String,
    pub description: Option<String>,
    pub severity: String,
    pub status: String,
    pub created_at: String,
}

#[derive(Serialize, FromRow)]
pub struct ChainStepRow {
    pub id: i64,
    pub chain_id: i64,
    pub finding_id: i64,
    pub step_order: i64,
    pub notes: Option<String>,
    pub finding_type: String,
    pub finding_severity: String,
    pub finding_status: String,
    pub finding_evidence: Option<String>,
}

#[derive(Deserialize)]
pub struct CreateChain {
    pub title: String,
    pub description: Option<String>,
    pub severity: String,
}

#[derive(Deserialize)]
pub struct AddStep {
    pub finding_id: i64,
    pub step_order: i64,
    pub notes: Option<String>,
}

pub async fn list(db: &SqlitePool, domain: &str) -> sqlx::Result<Vec<ChainRow>> {
    let sql = r#"
        SELECT ac.id, ac.target_id, ac.title, ac.description,
               ac.severity, ac.status, ac.created_at
        FROM attack_chains ac
        JOIN targets t ON t.id = ac.target_id
        WHERE t.domain = ?
        ORDER BY ac.created_at DESC
    "#;

    sqlx::query_as::<_, ChainRow>(sql)
        .bind(domain)
        .fetch_all(db)
        .await
}

pub async fn create(db: &SqlitePool, domain: &str, input: &CreateChain) -> sqlx::Result<i64> {
    sqlx::query("INSERT OR IGNORE INTO targets (domain) VALUES (?)")
        .bind(domain)
        .execute(db)
        .await?;

    let result = sqlx::query(
        r#"
        INSERT INTO attack_chains (target_id, title, description, severity)
        SELECT id, ?, ?, ? FROM targets WHERE domain = ?
    "#,
    )
    .bind(&input.title)
    .bind(&input.description)
    .bind(&input.severity)
    .bind(domain)
    .execute(db)
    .await?;

    Ok(result.last_insert_rowid())
}

pub async fn steps(db: &SqlitePool, chain_id: i64) -> sqlx::Result<Vec<ChainStepRow>> {
    let sql = r#"
        SELECT
            cs.id, cs.chain_id, cs.finding_id, cs.step_order, cs.notes,
            f.type  AS finding_type,
            f.severity AS finding_severity,
            f.status   AS finding_status,
            f.evidence AS finding_evidence
        FROM chain_steps cs
        JOIN findings f ON f.id = cs.finding_id
        WHERE cs.chain_id = ?
        ORDER BY cs.step_order
    "#;

    sqlx::query_as::<_, ChainStepRow>(sql)
        .bind(chain_id)
        .fetch_all(db)
        .await
}

pub async fn add_step(db: &SqlitePool, chain_id: i64, input: &AddStep) -> sqlx::Result<i64> {
    let result = sqlx::query(
        r#"
        INSERT INTO chain_steps (chain_id, finding_id, step_order, notes)
        VALUES (?, ?, ?, ?)
        ON CONFLICT(chain_id, step_order)
        DO UPDATE SET finding_id = excluded.finding_id,
                      notes      = excluded.notes
    "#,
    )
    .bind(chain_id)
    .bind(input.finding_id)
    .bind(input.step_order)
    .bind(&input.notes)
    .execute(db)
    .await?;

    Ok(result.last_insert_rowid())
}
