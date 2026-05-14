use serde::{Deserialize, Serialize};
use sqlx::{FromRow, SqlitePool};

#[derive(Serialize, FromRow)]
pub struct CredentialRow {
    pub id: i64,
    pub target_id: i64,
    pub r#type: String,
    pub username: Option<String>,
    pub secret: String,
    pub description: Option<String>,
    pub notes: Option<String>,
}

#[derive(Deserialize)]
pub struct CreateCredential {
    pub r#type: String,
    pub username: Option<String>,
    pub secret: String,
    pub description: Option<String>,
    pub notes: Option<String>,
}

pub async fn list(db: &SqlitePool, domain: &str) -> sqlx::Result<Vec<CredentialRow>> {
    let sql = r#"
        SELECT c.id, c.target_id, c.type, c.username,
               c.secret, c.description, c.notes
        FROM credentials c
        JOIN targets t ON t.id = c.target_id
        WHERE t.domain = ?
    "#;

    sqlx::query_as::<_, CredentialRow>(sql)
        .bind(domain)
        .fetch_all(db)
        .await
}

pub async fn create(db: &SqlitePool, domain: &str, input: &CreateCredential) -> sqlx::Result<i64> {
    sqlx::query("INSERT OR IGNORE INTO targets (domain) VALUES (?)")
        .bind(domain)
        .execute(db)
        .await?;

    let result = sqlx::query(
        r#"
        INSERT INTO credentials (target_id, type, username, secret, description, notes)
        SELECT id, ?, ?, ?, ?, ? FROM targets WHERE domain = ?
        ON CONFLICT(target_id, type, username, secret)
        DO UPDATE SET description = excluded.description,
                      notes       = excluded.notes
    "#,
    )
    .bind(&input.r#type)
    .bind(&input.username)
    .bind(&input.secret)
    .bind(&input.description)
    .bind(&input.notes)
    .bind(domain)
    .execute(db)
    .await?;

    Ok(result.last_insert_rowid())
}
