use serde::{Deserialize, Serialize};
use sqlx::{FromRow, SqlitePool};

#[derive(Serialize, FromRow)]
pub struct ProxyRow {
    pub id: i64,
    pub target_id: Option<i64>,
    pub url: String,
    pub r#type: String,
    pub active: i64,
    pub description: Option<String>,
    pub notes: Option<String>,
    pub created_at: String,
}

#[derive(Deserialize)]
pub struct CreateProxy {
    pub url: String,
    pub r#type: String,
    pub active: Option<i64>,
    pub description: Option<String>,
    pub notes: Option<String>,
}

pub async fn list(db: &SqlitePool, domain: &str) -> sqlx::Result<Vec<ProxyRow>> {
    let sql = r#"
        SELECT p.id, p.target_id, p.url, p.type, p.active, p.description, p.notes, p.created_at
        FROM proxies p
        JOIN targets t ON t.id = p.target_id
        WHERE t.domain = ?
        ORDER BY p.id ASC
    "#;
    sqlx::query_as::<_, ProxyRow>(sql)
        .bind(domain)
        .fetch_all(db)
        .await
}

pub async fn create(db: &SqlitePool, domain: &str, input: &CreateProxy) -> sqlx::Result<i64> {
    sqlx::query("INSERT OR IGNORE INTO targets (domain) VALUES (?)")
        .bind(domain)
        .execute(db)
        .await?;

    let result = sqlx::query(
        r#"
        INSERT INTO proxies (target_id, url, type, active, description, notes)
        SELECT id, ?, ?, ?, ?, ?
        FROM targets WHERE domain = ?
        "#,
    )
    .bind(&input.url)
    .bind(&input.r#type)
    .bind(input.active.unwrap_or(1))
    .bind(&input.description)
    .bind(&input.notes)
    .bind(domain)
    .execute(db)
    .await?;

    Ok(result.last_insert_rowid())
}
