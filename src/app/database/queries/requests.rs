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
    pub notes: Option<String>,
    pub created_at: String,
}

#[derive(Deserialize)]
pub struct CreateRequest {
    pub endpoint_id: i64,
    pub raw_request: String,
    pub raw_response: Option<String>,
    pub status_code: Option<i64>,
    pub response_time_ms: Option<i64>,
    pub notes: Option<String>,
}

pub async fn list(db: &SqlitePool, endpoint_id: i64) -> sqlx::Result<Vec<RequestRow>> {
    let sql = r#"
        SELECT id, endpoint_id, raw_request, raw_response,
               status_code, response_time_ms, notes, created_at
        FROM requests
        WHERE endpoint_id = ?
        ORDER BY created_at DESC
    "#;

    sqlx::query_as::<_, RequestRow>(sql)
        .bind(endpoint_id)
        .fetch_all(db)
        .await
}

pub async fn create(db: &SqlitePool, input: &CreateRequest) -> sqlx::Result<i64> {
    let sql = r#"
        INSERT INTO requests
            (endpoint_id, raw_request, raw_response, status_code, response_time_ms, notes)
        VALUES (?, ?, ?, ?, ?, ?)
    "#;

    let result = sqlx::query(sql)
        .bind(input.endpoint_id)
        .bind(&input.raw_request)
        .bind(&input.raw_response)
        .bind(input.status_code)
        .bind(input.response_time_ms)
        .bind(&input.notes)
        .execute(db)
        .await?;

    Ok(result.last_insert_rowid())
}
