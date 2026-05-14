use serde::{Deserialize, Serialize};
use sqlx::{FromRow, SqlitePool};

#[derive(Serialize, FromRow)]
pub struct EndpointRow {
    pub id: i64,
    pub method: String,
    pub path: String,
    pub status_code: Option<i64>,
    pub auth: bool,
    pub description: Option<String>,
    pub notes: Option<String>,
}

#[derive(Deserialize)]
pub struct CreateEndpoint {
    pub method: String,
    pub path: String,
    pub status_code: Option<i64>,
    pub auth: Option<bool>,
    pub description: Option<String>,
    pub notes: Option<String>,
}

pub async fn list(
    db: &SqlitePool,
    domain: &str,
    status: Option<i64>,
) -> sqlx::Result<Vec<EndpointRow>> {
    let sql = r#"
        SELECT e.id, e.method, e.path, e.status_code,
               e.auth,
               e.description, e.notes
        FROM endpoints e
        JOIN targets t ON t.id = e.target_id
        WHERE t.domain = ?
          AND (? IS NULL OR e.status_code = ?)
        ORDER BY e.path
    "#;

    sqlx::query_as::<_, EndpointRow>(sql)
        .bind(domain)
        .bind(status)
        .bind(status)
        .fetch_all(db)
        .await
}

pub async fn create(db: &SqlitePool, domain: &str, input: &CreateEndpoint) -> sqlx::Result<i64> {
    ensure_target(db, domain).await?;

    let result = sqlx::query(
        r#"
        INSERT INTO endpoints (target_id, method, path, status_code, auth, description, notes)
        SELECT id, ?, ?, ?, ?, ?, ? FROM targets WHERE domain = ?
        ON CONFLICT(target_id, method, path)
        DO UPDATE SET status_code = excluded.status_code,
                      auth        = excluded.auth,
                      description = excluded.description,
                      notes       = excluded.notes
    "#,
    )
    .bind(&input.method)
    .bind(&input.path)
    .bind(input.status_code)
    .bind(input.auth.unwrap_or(false))
    .bind(&input.description)
    .bind(&input.notes)
    .bind(domain)
    .execute(db)
    .await?;

    Ok(result.last_insert_rowid())
}

async fn ensure_target(db: &SqlitePool, domain: &str) -> sqlx::Result<()> {
    sqlx::query("INSERT OR IGNORE INTO targets (domain) VALUES (?)")
        .bind(domain)
        .execute(db)
        .await?;
    Ok(())
}
