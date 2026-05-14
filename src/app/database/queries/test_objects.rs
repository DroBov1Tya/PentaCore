use serde::{Deserialize, Serialize};
use sqlx::{FromRow, SqlitePool};

#[derive(Serialize, FromRow)]
pub struct TestObjectRow {
    pub id: i64,
    pub target_id: i64,
    pub object_type: String,
    pub object_id: String,
    pub description: Option<String>,
    pub rollback_method: Option<String>,
    pub rollback_url: Option<String>,
    pub rollback_body: Option<String>,
    pub status: String,
    pub created_at: String,
}

#[derive(Deserialize)]
pub struct ClaimTestObject {
    pub object_type: String,
    pub object_id: String,
    pub description: Option<String>,
    pub rollback_method: Option<String>,
    pub rollback_url: Option<String>,
    pub rollback_body: Option<String>,
}

pub async fn list(
    db: &SqlitePool,
    domain: &str,
    status: Option<&str>,
) -> sqlx::Result<Vec<TestObjectRow>> {
    sqlx::query_as::<_, TestObjectRow>(
        r#"
        SELECT o.id, o.target_id, o.object_type, o.object_id, o.description,
               o.rollback_method, o.rollback_url, o.rollback_body, o.status, o.created_at
        FROM test_objects o
        JOIN targets t ON t.id = o.target_id
        WHERE t.domain = ?
          AND (? IS NULL OR o.status = ?)
        ORDER BY o.created_at DESC
        "#,
    )
    .bind(domain)
    .bind(status)
    .bind(status)
    .fetch_all(db)
    .await
}

pub async fn claim(db: &SqlitePool, domain: &str, input: &ClaimTestObject) -> sqlx::Result<i64> {
    let result = sqlx::query(
        r#"
        INSERT INTO test_objects
            (target_id, object_type, object_id, description, rollback_method, rollback_url, rollback_body)
        VALUES (
            (SELECT id FROM targets WHERE domain = ?),
            ?, ?, ?, ?, ?, ?
        )
        "#,
    )
    .bind(domain)
    .bind(&input.object_type)
    .bind(&input.object_id)
    .bind(&input.description)
    .bind(&input.rollback_method)
    .bind(&input.rollback_url)
    .bind(&input.rollback_body)
    .execute(db)
    .await?;

    Ok(result.last_insert_rowid())
}

pub async fn rollback(db: &SqlitePool, id: i64) -> sqlx::Result<Option<TestObjectRow>> {
    let obj = sqlx::query_as::<_, TestObjectRow>("SELECT * FROM test_objects WHERE id = ?")
        .bind(id)
        .fetch_optional(db)
        .await?;

    if obj.is_some() {
        sqlx::query("UPDATE test_objects SET status = 'rolled_back' WHERE id = ?")
            .bind(id)
            .execute(db)
            .await?;
    }

    Ok(obj)
}
