use serde::{Deserialize, Serialize};
use sqlx::{FromRow, SqlitePool};

#[derive(Serialize, FromRow)]
pub struct RequestRow {
    pub id: i64,
    pub endpoint_id: i64,
    pub raw_request: String,
    pub raw_response: Option<String>,
    pub status_code: Option<i64>,
    pub response_time_ms: Option<i64>,
    pub description: Option<String>,
    pub notes: Option<String>,
    pub created_at: String,
}

#[derive(Deserialize)]
pub struct CreateRequest {
    pub raw_request: String,
    pub raw_response: Option<String>,
    pub status_code: Option<i64>,
    pub response_time_ms: Option<i64>,
    pub description: Option<String>,
    pub notes: Option<String>,
}

pub async fn list(db: &SqlitePool, endpoint_id: i64) -> sqlx::Result<Vec<RequestRow>> {
    let sql = r#"
        SELECT id, endpoint_id, raw_request, raw_response,
               status_code, response_time_ms, description, notes, created_at
        FROM requests
        WHERE endpoint_id = ?
        ORDER BY created_at DESC
    "#;

    sqlx::query_as::<_, RequestRow>(sql)
        .bind(endpoint_id)
        .fetch_all(db)
        .await
}

pub async fn create(db: &SqlitePool, endpoint_id: i64, input: &CreateRequest) -> sqlx::Result<i64> {
    let result = sqlx::query(
        r#"
        INSERT INTO requests
            (endpoint_id, raw_request, raw_response,
             status_code, response_time_ms, description, notes)
        VALUES (?, ?, ?, ?, ?, ?, ?)
    "#,
    )
    .bind(endpoint_id)
    .bind(&input.raw_request)
    .bind(&input.raw_response)
    .bind(input.status_code)
    .bind(input.response_time_ms)
    .bind(&input.description)
    .bind(&input.notes)
    .execute(db)
    .await?;

    Ok(result.last_insert_rowid())
}

pub async fn get(db: &SqlitePool, id: i64) -> sqlx::Result<Option<RequestRow>> {
    let sql = r#"
        SELECT id, endpoint_id, raw_request, raw_response,
               status_code, response_time_ms, description, notes, created_at
        FROM requests
        WHERE id = ?
    "#;

    sqlx::query_as::<_, RequestRow>(sql)
        .bind(id)
        .fetch_optional(db)
        .await
}
