use serde::Serialize;
use sqlx::{FromRow, SqlitePool};

#[derive(Serialize)]
pub struct AuditReport {
    pub domain: String,
    pub generated_at: String,
    pub scope: Option<ScopeInfo>,
    pub executive_summary: ExecutiveSummary,
    pub findings: Vec<FindingReport>,
    pub coverage_summary: CoverageSummary,
    pub appendix: Appendix,
}

#[derive(Serialize)]
pub struct ScopeInfo {
    pub objective: String,
    pub in_scope: String,
    pub out_of_scope: Option<String>,
    pub rules: Option<String>,
}

#[derive(Serialize)]
pub struct ExecutiveSummary {
    pub total_endpoints: i64,
    pub total_findings: i64,
    pub by_severity: SeverityCounts,
    pub by_status: StatusCounts,
    pub coverage_percent: f64,
    pub risk_rating: String,
}

#[derive(Serialize)]
pub struct SeverityCounts {
    pub critical: i64,
    pub high: i64,
    pub medium: i64,
    pub low: i64,
    pub info: i64,
}

#[derive(Serialize)]
pub struct StatusCounts {
    pub confirmed: i64,
    pub potential: i64,
    pub false_positive: i64,
}

#[derive(Serialize)]
pub struct FindingReport {
    pub id: i64,
    pub finding_type: String,
    pub severity: String,
    pub status: String,
    pub description: Option<String>,
    pub payload: Option<String>,
    pub evidence: Option<String>,
    pub endpoint_path: Option<String>,
    pub endpoint_method: Option<String>,
    pub raw_request: Option<String>,
    pub created_at: String,
}

#[derive(Serialize)]
pub struct CoverageSummary {
    pub total_vectors: i64,
    pub done: i64,
    pub pending: i64,
    pub skipped: i64,
    pub in_progress: i64,
    pub uncovered_endpoints: i64,
    pub vectors_breakdown: Vec<VectorBreakdown>,
}

#[derive(Serialize, FromRow)]
pub struct VectorBreakdown {
    pub vector: String,
    pub total: i64,
    pub done: i64,
    pub pending: i64,
}

#[derive(Serialize)]
pub struct Appendix {
    pub subdomains: Vec<String>,
    pub credentials_count: i64,
    pub test_objects_active: i64,
    pub total_requests: i64,
}

pub async fn generate(db: &SqlitePool, domain: &str) -> sqlx::Result<AuditReport> {
    let target_id: Option<i64> = sqlx::query_scalar("SELECT id FROM targets WHERE domain = ?")
        .bind(domain)
        .fetch_optional(db)
        .await?;

    let target_id = match target_id {
        Some(id) => id,
        None => {
            return Ok(AuditReport {
                domain: domain.to_string(),
                generated_at: chrono::Utc::now().to_rfc3339(),
                scope: None,
                executive_summary: ExecutiveSummary {
                    total_endpoints: 0,
                    total_findings: 0,
                    by_severity: SeverityCounts {
                        critical: 0,
                        high: 0,
                        medium: 0,
                        low: 0,
                        info: 0,
                    },
                    by_status: StatusCounts {
                        confirmed: 0,
                        potential: 0,
                        false_positive: 0,
                    },
                    coverage_percent: 0.0,
                    risk_rating: "N/A".into(),
                },
                findings: vec![],
                coverage_summary: CoverageSummary {
                    total_vectors: 0,
                    done: 0,
                    pending: 0,
                    skipped: 0,
                    in_progress: 0,
                    uncovered_endpoints: 0,
                    vectors_breakdown: vec![],
                },
                appendix: Appendix {
                    subdomains: vec![],
                    credentials_count: 0,
                    test_objects_active: 0,
                    total_requests: 0,
                },
            });
        }
    };

    let scope: Option<ScopeInfo> = sqlx::query_as::<_, (String, String, Option<String>, Option<String>)>(
        "SELECT objective, in_scope, out_of_scope, rules FROM scopes WHERE target_id = ? ORDER BY id DESC LIMIT 1"
    ).bind(target_id).fetch_optional(db).await?.map(|(obj, ins, oos, rules)| {
        ScopeInfo { objective: obj, in_scope: ins, out_of_scope: oos, rules }
    });

    let critical: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM findings WHERE target_id = ? AND severity = 'critical'",
    )
    .bind(target_id)
    .fetch_one(db)
    .await?;
    let high: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM findings WHERE target_id = ? AND severity = 'high'",
    )
    .bind(target_id)
    .fetch_one(db)
    .await?;
    let medium: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM findings WHERE target_id = ? AND severity = 'medium'",
    )
    .bind(target_id)
    .fetch_one(db)
    .await?;
    let low: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM findings WHERE target_id = ? AND severity = 'low'",
    )
    .bind(target_id)
    .fetch_one(db)
    .await?;
    let info: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM findings WHERE target_id = ? AND severity = 'info'",
    )
    .bind(target_id)
    .fetch_one(db)
    .await?;

    let confirmed: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM findings WHERE target_id = ? AND status = 'confirmed'",
    )
    .bind(target_id)
    .fetch_one(db)
    .await?;
    let potential: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM findings WHERE target_id = ? AND status = 'potential'",
    )
    .bind(target_id)
    .fetch_one(db)
    .await?;
    let false_positive: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM findings WHERE target_id = ? AND status = 'false_positive'",
    )
    .bind(target_id)
    .fetch_one(db)
    .await?;

    let total_endpoints: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM endpoints WHERE target_id = ?")
            .bind(target_id)
            .fetch_one(db)
            .await?;
    let total_findings = critical + high + medium + low + info;

    let coverage_done: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM coverage c JOIN endpoints e ON e.id = c.endpoint_id WHERE e.target_id = ? AND c.status = 'done'"
    ).bind(target_id).fetch_one(db).await?;
    let coverage_total: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM coverage c JOIN endpoints e ON e.id = c.endpoint_id WHERE e.target_id = ?"
    ).bind(target_id).fetch_one(db).await?;
    let coverage_pending: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM coverage c JOIN endpoints e ON e.id = c.endpoint_id WHERE e.target_id = ? AND c.status = 'pending'"
    ).bind(target_id).fetch_one(db).await?;
    let coverage_skipped: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM coverage c JOIN endpoints e ON e.id = c.endpoint_id WHERE e.target_id = ? AND c.status = 'skipped'"
    ).bind(target_id).fetch_one(db).await?;
    let coverage_in_progress: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM coverage c JOIN endpoints e ON e.id = c.endpoint_id WHERE e.target_id = ? AND c.status = 'in_progress'"
    ).bind(target_id).fetch_one(db).await?;
    let uncovered_endpoints: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM endpoints e WHERE e.target_id = ? AND NOT EXISTS (SELECT 1 FROM coverage c WHERE c.endpoint_id = e.id)"
    ).bind(target_id).fetch_one(db).await?;

    let coverage_percent = if coverage_total > 0 {
        (coverage_done as f64 / coverage_total as f64) * 100.0
    } else {
        0.0
    };

    let risk_rating = if critical > 0 {
        "CRITICAL"
    } else if high > 0 {
        "HIGH"
    } else if medium > 0 {
        "MEDIUM"
    } else if low > 0 {
        "LOW"
    } else {
        "INFO"
    };

    let findings: Vec<FindingReport> = sqlx::query_as::<
        _,
        (
            i64,
            String,
            String,
            String,
            Option<String>,
            Option<String>,
            Option<String>,
            Option<String>,
            Option<String>,
            Option<String>,
            String,
        ),
    >(
        r#"SELECT f.id, f.type, f.severity, f.status, f.description, f.payload, f.evidence,
                  e.path, e.method, f.raw_request, f.created_at
           FROM findings f
           LEFT JOIN endpoints e ON e.id = f.endpoint_id
           WHERE f.target_id = ?
           ORDER BY CASE f.severity
               WHEN 'critical' THEN 1 WHEN 'high' THEN 2 WHEN 'medium' THEN 3
               WHEN 'low' THEN 4 ELSE 5 END, f.id"#,
    )
    .bind(target_id)
    .fetch_all(db)
    .await?
    .into_iter()
    .map(|r| FindingReport {
        id: r.0,
        finding_type: r.1,
        severity: r.2,
        status: r.3,
        description: r.4,
        payload: r.5,
        evidence: r.6,
        endpoint_path: r.7,
        endpoint_method: r.8,
        raw_request: r.9,
        created_at: r.10,
    })
    .collect();

    let vectors_breakdown: Vec<VectorBreakdown> = sqlx::query_as::<_, VectorBreakdown>(
        r#"SELECT c.vector,
                  COUNT(*) as total,
                  SUM(CASE WHEN c.status = 'done' THEN 1 ELSE 0 END) as done,
                  SUM(CASE WHEN c.status = 'pending' THEN 1 ELSE 0 END) as pending
           FROM coverage c
           JOIN endpoints e ON e.id = c.endpoint_id
           WHERE e.target_id = ?
           GROUP BY c.vector
           ORDER BY pending DESC"#,
    )
    .bind(target_id)
    .fetch_all(db)
    .await?;

    let subdomains: Vec<String> = sqlx::query_scalar(
        "SELECT t2.domain FROM target_relations tr JOIN targets t2 ON t2.id = tr.to_id WHERE tr.from_id = ? AND tr.type = 'subdomain'"
    ).bind(target_id).fetch_all(db).await?;

    let credentials_count: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM credentials WHERE target_id = ?")
            .bind(target_id)
            .fetch_one(db)
            .await?;
    let test_objects_active: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM test_objects WHERE target_id = ? AND status = 'active'",
    )
    .bind(target_id)
    .fetch_one(db)
    .await?;
    let total_requests: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM requests r JOIN endpoints e ON e.id = r.endpoint_id WHERE e.target_id = ?"
    ).bind(target_id).fetch_one(db).await?;

    Ok(AuditReport {
        domain: domain.to_string(),
        generated_at: chrono::Utc::now().to_rfc3339(),
        scope,
        executive_summary: ExecutiveSummary {
            total_endpoints,
            total_findings,
            by_severity: SeverityCounts {
                critical,
                high,
                medium,
                low,
                info,
            },
            by_status: StatusCounts {
                confirmed,
                potential,
                false_positive,
            },
            coverage_percent,
            risk_rating: risk_rating.into(),
        },
        findings,
        coverage_summary: CoverageSummary {
            total_vectors: coverage_total,
            done: coverage_done,
            pending: coverage_pending,
            skipped: coverage_skipped,
            in_progress: coverage_in_progress,
            uncovered_endpoints,
            vectors_breakdown,
        },
        appendix: Appendix {
            subdomains,
            credentials_count,
            test_objects_active,
            total_requests,
        },
    })
}
