use serde::{Deserialize, Serialize};
use sqlx::{FromRow, SqlitePool};

#[derive(Serialize, FromRow)]
pub struct FindingRow {
    pub id: i64,
    pub endpoint_id: Option<i64>,
    pub request_id: Option<i64>,
    pub parent_id: Option<i64>,
    pub r#type: String,
    pub severity: String,
    pub status: String,
    pub raw_request: Option<String>,
    pub payload: Option<String>,
    pub evidence: Option<String>,
    pub description: Option<String>,
    pub created_at: String,
}

#[derive(Deserialize)]
pub struct CreateFinding {
    pub endpoint_id: Option<i64>,
    pub request_id: Option<i64>,
    pub parent_id: Option<i64>,
    pub r#type: String,
    pub severity: String,
    pub status: Option<String>,
    pub raw_request: Option<String>,
    pub payload: Option<String>,
    pub evidence: Option<String>,
    pub description: Option<String>,
}

pub async fn list(
    db: &SqlitePool,
    domain: &str,
    severity: Option<&str>,
    status: Option<&str>,
) -> sqlx::Result<Vec<FindingRow>> {
    let sql = r#"
        SELECT f.id, f.endpoint_id, f.request_id, f.parent_id,
               f.type, f.severity, f.status,
               f.raw_request, f.payload, f.evidence,
               f.description, f.created_at
        FROM findings f
        JOIN targets t ON t.id = f.target_id
        WHERE t.domain = ?
          AND (? IS NULL OR f.severity = ?)
          AND (? IS NULL OR f.status   = ?)
        ORDER BY
            CASE f.severity
                WHEN 'critical' THEN 1
                WHEN 'high'     THEN 2
                WHEN 'medium'   THEN 3
                WHEN 'low'      THEN 4
                ELSE                 5
            END
    "#;

    sqlx::query_as::<_, FindingRow>(sql)
        .bind(domain)
        .bind(severity)
        .bind(severity)
        .bind(status)
        .bind(status)
        .fetch_all(db)
        .await
}

pub async fn create(db: &SqlitePool, domain: &str, input: &CreateFinding) -> sqlx::Result<i64> {
    sqlx::query("INSERT OR IGNORE INTO targets (domain) VALUES (?)")
        .bind(domain)
        .execute(db)
        .await?;

    let status = input.status.as_deref().unwrap_or("potential");

    let result = sqlx::query(
        r#"
        INSERT INTO findings
            (target_id, endpoint_id, request_id, parent_id,
             type, severity, status, raw_request, payload, evidence, description)
        SELECT id, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?
        FROM targets WHERE domain = ?
    "#,
    )
    .bind(input.endpoint_id)
    .bind(input.request_id)
    .bind(input.parent_id)
    .bind(&input.r#type)
    .bind(&input.severity)
    .bind(status)
    .bind(&input.raw_request)
    .bind(&input.payload)
    .bind(&input.evidence)
    .bind(&input.description)
    .bind(domain)
    .execute(db)
    .await?;

    Ok(result.last_insert_rowid())
}
