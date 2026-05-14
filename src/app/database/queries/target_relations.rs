use serde::{Deserialize, Serialize};
use sqlx::{FromRow, SqlitePool};

#[derive(Serialize, FromRow)]
pub struct RelationRow {
    pub id: i64,
    pub from_domain: String,
    pub to_domain: String,
    pub rel_type: String,
    pub description: Option<String>,
    pub created_at: String,
}

#[derive(Deserialize)]
pub struct CreateRelation {
    pub to_domain: String,
    pub rel_type: String,
    pub description: Option<String>,
}

pub async fn list(db: &SqlitePool, domain: &str) -> sqlx::Result<Vec<RelationRow>> {
    let sql = r#"
        SELECT
            r.id,
            t_from.domain AS from_domain,
            t_to.domain   AS to_domain,
            r.type        AS rel_type,
            r.description,
            r.created_at
        FROM target_relations r
        JOIN targets t_from ON t_from.id = r.from_id
        JOIN targets t_to   ON t_to.id   = r.to_id
        WHERE t_from.domain = ? OR t_to.domain = ?
        ORDER BY r.created_at DESC
    "#;

    sqlx::query_as::<_, RelationRow>(sql)
        .bind(domain)
        .bind(domain)
        .fetch_all(db)
        .await
}

pub async fn create(
    db: &SqlitePool,
    from_domain: &str,
    input: &CreateRelation,
) -> sqlx::Result<i64> {
    sqlx::query("INSERT OR IGNORE INTO targets (domain) VALUES (?)")
        .bind(from_domain)
        .execute(db)
        .await?;

    sqlx::query("INSERT OR IGNORE INTO targets (domain) VALUES (?)")
        .bind(&input.to_domain)
        .execute(db)
        .await?;

    let result = sqlx::query(
        r#"
        INSERT INTO target_relations (from_id, to_id, type, description)
        SELECT f.id, t.id, ?, ?
        FROM targets f, targets t
        WHERE f.domain = ? AND t.domain = ?
        ON CONFLICT(from_id, to_id, type)
        DO UPDATE SET description = excluded.description
    "#,
    )
    .bind(&input.rel_type)
    .bind(&input.description)
    .bind(from_domain)
    .bind(&input.to_domain)
    .execute(db)
    .await?;

    Ok(result.last_insert_rowid())
}
