use serde::{Deserialize, Serialize};
use sqlx::{FromRow, SqlitePool};

#[derive(Serialize, FromRow)]
pub struct ScopeRow {
    pub id: i64,
    pub target_id: i64,
    pub objective: String,
    pub in_scope: String,
    pub out_of_scope: Option<String>,
    pub rules: Option<String>,
    pub created_at: String,
}

#[derive(Deserialize)]
pub struct CreateScope {
    pub objective: String,
    pub in_scope: String,
    pub out_of_scope: Option<String>,
    pub rules: Option<String>,
    pub domain_type: Option<String>,
}

pub async fn get(db: &SqlitePool, domain: &str) -> sqlx::Result<Option<ScopeRow>> {
    let sql = r#"
        SELECT s.id, s.target_id, s.objective, s.in_scope,
               s.out_of_scope, s.rules, s.created_at
        FROM scopes s
        JOIN targets t ON t.id = s.target_id
        WHERE t.domain = ?
        ORDER BY s.created_at DESC
        LIMIT 1
    "#;

    sqlx::query_as::<_, ScopeRow>(sql)
        .bind(domain)
        .fetch_optional(db)
        .await
}

pub async fn upsert(db: &SqlitePool, domain: &str, input: &CreateScope) -> sqlx::Result<i64> {
    sqlx::query("INSERT OR IGNORE INTO targets (domain) VALUES (?)")
        .bind(domain)
        .execute(db)
        .await?;
    sqlx::query(
        r#"
        DELETE FROM scopes
        WHERE target_id = (SELECT id FROM targets WHERE domain = ?)
    "#,
    )
    .bind(domain)
    .execute(db)
    .await?;

    let result = sqlx::query(
        r#"
        INSERT INTO scopes (target_id, objective, in_scope, out_of_scope, rules, domain_type)
        SELECT id, ?, ?, ?, ?, ? FROM targets WHERE domain = ?
    "#,
    )
    .bind(&input.objective)
    .bind(&input.in_scope)
    .bind(&input.out_of_scope)
    .bind(&input.rules)
    .bind(&input.domain_type)
    .bind(domain)
    .execute(db)
    .await?;

    Ok(result.last_insert_rowid())
}
