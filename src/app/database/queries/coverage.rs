use serde::{Deserialize, Serialize};
use sqlx::{FromRow, SqlitePool};

#[derive(Serialize, FromRow)]
pub struct CoverageRow {
    pub id: i64,
    pub endpoint_id: i64,
    pub vector: String,
    pub status: String,
    pub description: Option<String>,
    pub notes: Option<String>,
    pub updated_at: String,
}

#[derive(Deserialize)]
pub struct UpsertCoverage {
    pub vector: String,
    pub status: String,
    pub description: Option<String>,
    pub notes: Option<String>,
}

pub async fn list(
    db: &SqlitePool,
    endpoint_id: i64,
    status: Option<&str>,
) -> sqlx::Result<Vec<CoverageRow>> {
    let sql = r#"
        SELECT id, endpoint_id, vector, status, description, notes, updated_at
        FROM coverage
        WHERE endpoint_id = ?
          AND (? IS NULL OR status = ?)
        ORDER BY vector
    "#;

    sqlx::query_as::<_, CoverageRow>(sql)
        .bind(endpoint_id)
        .bind(status)
        .bind(status)
        .fetch_all(db)
        .await
}

pub async fn upsert(
    db: &SqlitePool,
    endpoint_id: i64,
    input: &UpsertCoverage,
) -> sqlx::Result<i64> {
    let result = sqlx::query(
        r#"
        INSERT INTO coverage (endpoint_id, vector, status, description, notes, updated_at)
        VALUES (?, ?, ?, ?, ?, CURRENT_TIMESTAMP)
        ON CONFLICT(endpoint_id, vector)
        DO UPDATE SET status      = excluded.status,
                      description = excluded.description,
                      notes       = excluded.notes,
                      updated_at  = CURRENT_TIMESTAMP
    "#,
    )
    .bind(endpoint_id)
    .bind(&input.vector)
    .bind(&input.status)
    .bind(&input.description)
    .bind(&input.notes)
    .execute(db)
    .await?;

    Ok(result.last_insert_rowid())
}
