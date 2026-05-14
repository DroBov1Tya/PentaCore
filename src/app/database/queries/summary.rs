use serde::Serialize;
use sqlx::{FromRow, SqlitePool};

#[derive(Serialize, FromRow)]
pub struct DomainSummary {
    pub domain: String,
    pub endpoints_total: i64,
    pub endpoints_200: i64,
    pub findings_total: i64,
    pub critical: i64,
    pub high: i64,
    pub medium: i64,
    pub low: i64,
    pub confirmed: i64,
    pub potential: i64,
}

pub async fn get(db: &SqlitePool, domain: &str) -> sqlx::Result<Option<DomainSummary>> {
    let sql = r#"
        SELECT
            t.domain,
            COUNT(DISTINCT e.id)                                             AS endpoints_total,
            COUNT(DISTINCT CASE WHEN e.status_code = 200  THEN e.id END)    AS endpoints_200,
            COUNT(DISTINCT f.id)                                             AS findings_total,
            COUNT(DISTINCT CASE WHEN f.severity = 'critical' THEN f.id END) AS critical,
            COUNT(DISTINCT CASE WHEN f.severity = 'high'     THEN f.id END) AS high,
            COUNT(DISTINCT CASE WHEN f.severity = 'medium'   THEN f.id END) AS medium,
            COUNT(DISTINCT CASE WHEN f.severity = 'low'      THEN f.id END) AS low,
            COUNT(DISTINCT CASE WHEN f.status = 'confirmed'  THEN f.id END) AS confirmed,
            COUNT(DISTINCT CASE WHEN f.status = 'potential'  THEN f.id END) AS potential
        FROM targets t
        LEFT JOIN endpoints e ON e.target_id = t.id
        LEFT JOIN findings  f ON f.target_id = t.id
        WHERE t.domain = ?
        GROUP BY t.domain
    "#;

    sqlx::query_as::<_, DomainSummary>(sql)
        .bind(domain)
        .fetch_optional(db)
        .await
}
