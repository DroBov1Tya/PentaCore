use serde::{Deserialize, Serialize};
use sqlx::{FromRow, SqlitePool};

#[derive(Serialize, FromRow)]
pub struct EndpointExampleRow {
    pub id: i64,
    pub endpoint_id: i64,
    pub raw_request: String,
    pub raw_response: Option<String>,
    pub status_code: Option<i64>,
    pub description: Option<String>,
    pub created_at: String,
}

#[derive(Deserialize)]
pub struct SaveExample {
    pub raw_request: String,
    pub raw_response: Option<String>,
    pub status_code: Option<i64>,
    pub description: Option<String>,
}

pub async fn get(db: &SqlitePool, endpoint_id: i64) -> sqlx::Result<Option<EndpointExampleRow>> {
    sqlx::query_as::<_, EndpointExampleRow>(
        r#"
        SELECT id, endpoint_id, raw_request, raw_response, status_code, description, created_at
        FROM endpoint_examples
        WHERE endpoint_id = ?
        "#,
    )
    .bind(endpoint_id)
    .fetch_optional(db)
    .await
}

pub async fn upsert(db: &SqlitePool, endpoint_id: i64, input: &SaveExample) -> sqlx::Result<i64> {
    let result = sqlx::query(
        r#"
        INSERT INTO endpoint_examples
            (endpoint_id, raw_request, raw_response, status_code, description)
        VALUES (?, ?, ?, ?, ?)
        ON CONFLICT(endpoint_id)
        DO UPDATE SET raw_request  = excluded.raw_request,
                      raw_response = excluded.raw_response,
                      status_code  = excluded.status_code,
                      description  = excluded.description,
                      created_at   = CURRENT_TIMESTAMP
        "#,
    )
    .bind(endpoint_id)
    .bind(&input.raw_request)
    .bind(&input.raw_response)
    .bind(input.status_code)
    .bind(&input.description)
    .execute(db)
    .await?;

    Ok(result.last_insert_rowid())
}
