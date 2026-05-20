use serde::Serialize;
use sqlx::SqlitePool;

use super::methodology;

/// Full 7-phase PTES methodology state machine.
/// Phases: setup → recon → enumeration → vuln_mapping → exploitation → post_exploitation → reporting

const PHASES: &[&str] = &[
    "setup",
    "recon",
    "enumeration",
    "vuln_mapping",
    "exploitation",
    "post_exploitation",
    "reporting",
];

#[derive(Serialize)]
pub struct PhasePlaybook {
    pub current_phase: String,
    pub phase_index: usize,
    pub phase_description: String,
    pub required_artifacts: Vec<String>,
    pub missing_artifacts: Vec<String>,
    pub transition_options: Vec<Transition>,
    pub open_hypotheses_count: i64,
    pub dead_ends_count: i64,
    pub phase_checklist: Vec<ChecklistItem>,
    /// Lessons from past engagements matching current situation (populated externally via RAG)
    pub recalled_lessons: Vec<serde_json::Value>,
}

#[derive(Serialize)]
pub struct Transition {
    pub to_phase: String,
    pub conditions_met: bool,
    pub missing: Vec<String>,
}

#[derive(Serialize)]
pub struct ChecklistItem {
    pub action: String,
    pub reason: String,
    pub done: bool,
}

#[derive(Serialize)]
pub struct TransitionResult {
    pub success: bool,
    pub from_phase: String,
    pub to_phase: String,
    pub message: String,
}

/// Determines current phase from DB state — either from explicit phase_transitions
/// table or by inferring from engagement artifacts (backward-compatible fallback).
async fn determine_phase(db: &SqlitePool, target_id: i64) -> sqlx::Result<String> {
    // First: check explicit phase transitions
    if let Some(phase) = methodology::get_current_phase(db, target_id).await? {
        return Ok(phase);
    }

    // Fallback: infer from engagement state (backward compatibility with existing data)
    let has_scope: bool =
        sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM scopes WHERE target_id = ?")
            .bind(target_id)
            .fetch_one(db)
            .await?
            > 0;

    if !has_scope {
        return Ok("setup".to_string());
    }

    let total_endpoints: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM endpoints WHERE target_id = ?")
            .bind(target_id)
            .fetch_one(db)
            .await?;

    if total_endpoints == 0 {
        return Ok("recon".to_string());
    }

    let untested: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM endpoints e WHERE e.target_id = ? AND NOT EXISTS (SELECT 1 FROM coverage c WHERE c.endpoint_id = e.id)"
    ).bind(target_id).fetch_one(db).await?;

    if untested > total_endpoints / 2 {
        return Ok("enumeration".to_string());
    }

    let potential: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM findings WHERE target_id = ? AND status = 'potential'",
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

    if potential > 0 && confirmed == 0 {
        return Ok("vuln_mapping".to_string());
    }

    if confirmed > 0 {
        let coverage_pending: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM coverage c JOIN endpoints e ON e.id = c.endpoint_id WHERE e.target_id = ? AND c.status = 'pending'"
        ).bind(target_id).fetch_one(db).await?;

        if coverage_pending > 0 {
            return Ok("exploitation".to_string());
        }
        return Ok("reporting".to_string());
    }

    Ok("vuln_mapping".to_string())
}

pub async fn get_playbook(db: &SqlitePool, domain: &str) -> sqlx::Result<PhasePlaybook> {
    let target_id: Option<i64> = sqlx::query_scalar("SELECT id FROM targets WHERE domain = ?")
        .bind(domain)
        .fetch_optional(db)
        .await?;

    let target_id = match target_id {
        Some(id) => id,
        None => {
            return Ok(PhasePlaybook {
                current_phase: "setup".to_string(),
                phase_index: 0,
                phase_description: "Target not registered. Define scope to begin.".to_string(),
                required_artifacts: vec!["scope definition".to_string()],
                missing_artifacts: vec!["scope definition".to_string()],
                transition_options: vec![],
                open_hypotheses_count: 0,
                dead_ends_count: 0,
                phase_checklist: vec![ChecklistItem {
                    action: "save_scope to register target and define engagement rules".to_string(),
                    reason: "Cannot start without authorization boundaries".to_string(),
                    done: false,
                }],
                recalled_lessons: vec![],
            });
        }
    };

    let current_phase = determine_phase(db, target_id).await?;
    let phase_index = PHASES.iter().position(|p| *p == current_phase).unwrap_or(0);

    let phase_description = match current_phase.as_str() {
        "setup" => "Define engagement scope, rules, and authorization boundaries.",
        "recon" => {
            "Passive and active reconnaissance. Discover infrastructure, subdomains, technologies, and entry points."
        }
        "enumeration" => {
            "Deep enumeration of discovered services. Map endpoints, parameters, authentication flows, and API schemas."
        }
        "vuln_mapping" => {
            "Map potential vulnerabilities to discovered attack surface. Test each vector systematically."
        }
        "exploitation" => {
            "Exploit confirmed vulnerabilities. Build PoCs, validate impact, chain findings."
        }
        "post_exploitation" => {
            "Pivot from initial access. Enumerate internal resources, escalate privileges, assess blast radius."
        }
        "reporting" => "Compile findings, verify evidence, generate final report.",
        _ => "Unknown phase.",
    };

    // Gather state for guard evaluation
    let has_scope: bool =
        sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM scopes WHERE target_id = ?")
            .bind(target_id)
            .fetch_one(db)
            .await?
            > 0;
    let total_endpoints: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM endpoints WHERE target_id = ?")
            .bind(target_id)
            .fetch_one(db)
            .await?;
    let total_subdomains: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM target_relations WHERE from_id = ? AND type = 'subdomain'",
    )
    .bind(target_id)
    .fetch_one(db)
    .await?;
    let untested_endpoints: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM endpoints e WHERE e.target_id = ? AND NOT EXISTS (SELECT 1 FROM coverage c WHERE c.endpoint_id = e.id)"
    ).bind(target_id).fetch_one(db).await?;
    let potential_findings: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM findings WHERE target_id = ? AND status = 'potential'",
    )
    .bind(target_id)
    .fetch_one(db)
    .await?;
    let confirmed_findings: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM findings WHERE target_id = ? AND status = 'confirmed'",
    )
    .bind(target_id)
    .fetch_one(db)
    .await?;
    let coverage_pending: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM coverage c JOIN endpoints e ON e.id = c.endpoint_id WHERE e.target_id = ? AND c.status = 'pending'"
    ).bind(target_id).fetch_one(db).await?;
    let coverage_done: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM coverage c JOIN endpoints e ON e.id = c.endpoint_id WHERE e.target_id = ? AND c.status = 'done'"
    ).bind(target_id).fetch_one(db).await?;
    let active_test_objects: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM test_objects WHERE target_id = ? AND status = 'active'",
    )
    .bind(target_id)
    .fetch_one(db)
    .await?;
    let no_evidence_confirmed: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM findings WHERE target_id = ? AND status = 'confirmed' AND (evidence IS NULL OR evidence = '')"
    ).bind(target_id).fetch_one(db).await?;

    let open_hypotheses_count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM hypotheses WHERE target_id = ? AND status IN ('open', 'testing')",
    )
    .bind(target_id)
    .fetch_one(db)
    .await?;
    let dead_ends_count: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM dead_ends WHERE target_id = ?")
            .bind(target_id)
            .fetch_one(db)
            .await?;

    // Required artifacts and missing check per phase
    let (required_artifacts, missing_artifacts) = match current_phase.as_str() {
        "setup" => {
            let req = vec!["scope definition".to_string()];
            let miss: Vec<String> = if !has_scope {
                vec!["scope definition".to_string()]
            } else {
                vec![]
            };
            (req, miss)
        }
        "recon" => {
            let req = vec![
                "subdomain enumeration".to_string(),
                "DNS resolution".to_string(),
                "≥1 discovered endpoint".to_string(),
            ];
            let mut miss = vec![];
            if total_subdomains == 0 {
                miss.push("subdomain enumeration".to_string());
            }
            if total_endpoints == 0 {
                miss.push("≥1 discovered endpoint".to_string());
            }
            (req, miss)
        }
        "enumeration" => {
            let req = vec![
                "all endpoints mapped".to_string(),
                "API specs parsed".to_string(),
                "auth flows identified".to_string(),
                "coverage entries created".to_string(),
            ];
            let mut miss = vec![];
            if untested_endpoints > 0 {
                miss.push(format!(
                    "{} endpoints lack coverage entries",
                    untested_endpoints
                ));
            }
            (req, miss)
        }
        "vuln_mapping" => {
            let req = vec![
                "each vector tested per endpoint".to_string(),
                "potential findings recorded".to_string(),
            ];
            let mut miss = vec![];
            if coverage_pending > 0 {
                miss.push(format!("{} vectors still pending", coverage_pending));
            }
            (req, miss)
        }
        "exploitation" => {
            let req = vec![
                "PoC for each potential finding".to_string(),
                "findings promoted to confirmed or rejected".to_string(),
            ];
            let mut miss = vec![];
            if potential_findings > 0 {
                miss.push(format!(
                    "{} findings still 'potential' — need PoC validation",
                    potential_findings
                ));
            }
            (req, miss)
        }
        "post_exploitation" => {
            let req = vec![
                "lateral movement attempted".to_string(),
                "privilege escalation checked".to_string(),
                "blast radius assessed".to_string(),
            ];
            (req, vec![])
        }
        "reporting" => {
            let req = vec![
                "all findings have evidence".to_string(),
                "test objects cleaned up".to_string(),
                "report generated".to_string(),
            ];
            let mut miss = vec![];
            if no_evidence_confirmed > 0 {
                miss.push(format!(
                    "{} confirmed findings lack evidence",
                    no_evidence_confirmed
                ));
            }
            if active_test_objects > 0 {
                miss.push(format!(
                    "{} test objects still active — rollback needed",
                    active_test_objects
                ));
            }
            (req, miss)
        }
        _ => (vec![], vec![]),
    };

    // Transition options with guards
    let mut transitions = vec![];

    if phase_index > 0 {
        // Can always go back
        transitions.push(Transition {
            to_phase: PHASES[phase_index - 1].to_string(),
            conditions_met: true,
            missing: vec![],
        });
    }

    if phase_index < PHASES.len() - 1 {
        let next = PHASES[phase_index + 1];
        let (can_advance, blockers) = check_transition_guards(
            &current_phase,
            next,
            has_scope,
            total_endpoints,
            total_subdomains,
            untested_endpoints,
            potential_findings,
            confirmed_findings,
            coverage_pending,
            coverage_done,
            active_test_objects,
            no_evidence_confirmed,
        );
        transitions.push(Transition {
            to_phase: next.to_string(),
            conditions_met: can_advance,
            missing: blockers,
        });
    }

    // Special: can always jump to reporting from exploitation+
    if phase_index >= 4 && phase_index < PHASES.len() - 1 {
        let mut blockers = vec![];
        if no_evidence_confirmed > 0 {
            blockers.push(format!(
                "{} confirmed findings lack evidence",
                no_evidence_confirmed
            ));
        }
        if active_test_objects > 0 {
            blockers.push(format!("{} test objects still active", active_test_objects));
        }
        transitions.push(Transition {
            to_phase: "reporting".to_string(),
            conditions_met: blockers.is_empty(),
            missing: blockers,
        });
    }

    // Phase-specific checklist
    let phase_checklist = build_checklist(
        &current_phase,
        has_scope,
        total_endpoints,
        total_subdomains,
        untested_endpoints,
        potential_findings,
        confirmed_findings,
        coverage_pending,
        coverage_done,
        active_test_objects,
        no_evidence_confirmed,
        open_hypotheses_count,
    );

    Ok(PhasePlaybook {
        current_phase,
        phase_index,
        phase_description: phase_description.to_string(),
        required_artifacts,
        missing_artifacts,
        transition_options: transitions,
        open_hypotheses_count,
        dead_ends_count,
        phase_checklist,
        recalled_lessons: vec![], // Populated externally by handler via RAG
    })
}

#[allow(clippy::too_many_arguments)]
fn check_transition_guards(
    from: &str,
    to: &str,
    has_scope: bool,
    total_endpoints: i64,
    total_subdomains: i64,
    untested_endpoints: i64,
    potential_findings: i64,
    _confirmed_findings: i64,
    coverage_pending: i64,
    _coverage_done: i64,
    active_test_objects: i64,
    no_evidence_confirmed: i64,
) -> (bool, Vec<String>) {
    let mut blockers = vec![];

    match (from, to) {
        ("setup", "recon") => {
            if !has_scope {
                blockers.push("Scope must be defined before recon".to_string());
            }
        }
        ("recon", "enumeration") => {
            if total_endpoints == 0 {
                blockers.push("At least 1 endpoint must be discovered".to_string());
            }
            if total_subdomains == 0 {
                blockers.push("Subdomain enumeration should be attempted".to_string());
            }
        }
        ("enumeration", "vuln_mapping") => {
            if untested_endpoints > total_endpoints / 3 {
                blockers.push(format!(
                    "{} endpoints lack coverage entries — create them first",
                    untested_endpoints
                ));
            }
        }
        ("vuln_mapping", "exploitation") => {
            if coverage_pending > 0 {
                blockers.push(format!(
                    "{} vectors still pending testing",
                    coverage_pending
                ));
            }
        }
        ("exploitation", "post_exploitation") => {
            if potential_findings > 0 {
                blockers.push(format!(
                    "{} findings are 'potential' — validate before pivoting",
                    potential_findings
                ));
            }
        }
        ("post_exploitation", "reporting") | (_, "reporting") => {
            if no_evidence_confirmed > 0 {
                blockers.push(format!(
                    "{} confirmed findings lack evidence",
                    no_evidence_confirmed
                ));
            }
            if active_test_objects > 0 {
                blockers.push(format!(
                    "{} test objects still active — cleanup required",
                    active_test_objects
                ));
            }
        }
        _ => {}
    }

    (blockers.is_empty(), blockers)
}

pub async fn transition(
    db: &SqlitePool,
    domain: &str,
    to_phase: &str,
    reason: Option<&str>,
) -> sqlx::Result<TransitionResult> {
    let target_id: Option<i64> = sqlx::query_scalar("SELECT id FROM targets WHERE domain = ?")
        .bind(domain)
        .fetch_optional(db)
        .await?;

    let target_id = match target_id {
        Some(id) => id,
        None => {
            return Ok(TransitionResult {
                success: false,
                from_phase: "unknown".to_string(),
                to_phase: to_phase.to_string(),
                message: "Target not found".to_string(),
            });
        }
    };

    if !PHASES.contains(&to_phase) {
        return Ok(TransitionResult {
            success: false,
            from_phase: "unknown".to_string(),
            to_phase: to_phase.to_string(),
            message: format!(
                "Invalid phase '{}'. Valid phases: {}",
                to_phase,
                PHASES.join(", ")
            ),
        });
    }

    let current = determine_phase(db, target_id).await?;

    methodology::save_phase_transition(db, target_id, &current, to_phase, reason).await?;

    Ok(TransitionResult {
        success: true,
        from_phase: current,
        to_phase: to_phase.to_string(),
        message: reason
            .map(|r| format!("Phase transitioned. Reason: {}", r))
            .unwrap_or_else(|| "Phase transitioned successfully.".to_string()),
    })
}

#[allow(clippy::too_many_arguments)]
fn build_checklist(
    phase: &str,
    has_scope: bool,
    total_endpoints: i64,
    total_subdomains: i64,
    untested_endpoints: i64,
    potential_findings: i64,
    confirmed_findings: i64,
    coverage_pending: i64,
    _coverage_done: i64,
    active_test_objects: i64,
    no_evidence_confirmed: i64,
    open_hypotheses: i64,
) -> Vec<ChecklistItem> {
    let mut items = vec![];

    match phase {
        "setup" => {
            items.push(ChecklistItem {
                action: "Define scope with save_scope".to_string(),
                reason: "Engagement rules and boundaries".to_string(),
                done: has_scope,
            });
        }
        "recon" => {
            items.push(ChecklistItem {
                action: "Run enumerate_subdomains".to_string(),
                reason: "Discover hidden infrastructure".to_string(),
                done: total_subdomains > 0,
            });
            items.push(ChecklistItem {
                action: "Run resolve_dns".to_string(),
                reason: "Map IP addresses, MX, TXT records".to_string(),
                done: total_subdomains > 0, // proxy signal
            });
            items.push(ChecklistItem {
                action: "Discover endpoints (manual crawl or parse_api_spec)".to_string(),
                reason: "Build attack surface map".to_string(),
                done: total_endpoints > 0,
            });
            items.push(ChecklistItem {
                action: "Check for /swagger.json, /openapi.json, /graphql".to_string(),
                reason: "API spec parsing gives full endpoint inventory".to_string(),
                done: total_endpoints > 5, // heuristic: spec usually gives many endpoints
            });
        }
        "enumeration" => {
            items.push(ChecklistItem {
                action: "Create coverage entries for all endpoints".to_string(),
                reason: "Track which vectors are tested per endpoint".to_string(),
                done: untested_endpoints == 0,
            });
            items.push(ChecklistItem {
                action: "Identify authentication flows".to_string(),
                reason: "Map auth mechanisms for later bypass testing".to_string(),
                done: false, // can't auto-detect
            });
            items.push(ChecklistItem {
                action: "Save endpoint examples for key endpoints".to_string(),
                reason: "Baseline valid request/response pairs".to_string(),
                done: false,
            });
            items.push(ChecklistItem {
                action: "Document hypotheses for promising attack vectors".to_string(),
                reason: "Structured tracking prevents lost insights".to_string(),
                done: open_hypotheses > 0,
            });
        }
        "vuln_mapping" => {
            items.push(ChecklistItem {
                action: format!("Test {} pending coverage vectors", coverage_pending),
                reason: "Systematic vector testing".to_string(),
                done: coverage_pending == 0,
            });
            items.push(ChecklistItem {
                action: "Check for IDOR/BOLA on auth-required endpoints".to_string(),
                reason: "Highest value targets for access control bypasses".to_string(),
                done: false,
            });
            items.push(ChecklistItem {
                action: "Record findings for any anomalies (status=potential)".to_string(),
                reason: "Capture suspicions immediately, validate later".to_string(),
                done: potential_findings > 0 || confirmed_findings > 0,
            });
        }
        "exploitation" => {
            items.push(ChecklistItem {
                action: format!(
                    "Validate {} potential findings with PoC",
                    potential_findings
                ),
                reason: "Promote to confirmed or reject as false_positive".to_string(),
                done: potential_findings == 0,
            });
            items.push(ChecklistItem {
                action: "Build attack chains for related findings".to_string(),
                reason: "Chained vulnerabilities increase severity".to_string(),
                done: confirmed_findings > 1,
            });
        }
        "post_exploitation" => {
            items.push(ChecklistItem {
                action: "Attempt lateral movement from confirmed access".to_string(),
                reason: "Assess blast radius".to_string(),
                done: false,
            });
            items.push(ChecklistItem {
                action: "Check privilege escalation paths".to_string(),
                reason: "Demonstrate full impact".to_string(),
                done: false,
            });
        }
        "reporting" => {
            items.push(ChecklistItem {
                action: format!(
                    "Add evidence to {} confirmed findings without it",
                    no_evidence_confirmed
                ),
                reason: "Reports without evidence are rejected".to_string(),
                done: no_evidence_confirmed == 0,
            });
            items.push(ChecklistItem {
                action: format!("Rollback {} active test objects", active_test_objects),
                reason: "Responsible disclosure requires cleanup".to_string(),
                done: active_test_objects == 0,
            });
            items.push(ChecklistItem {
                action: "Generate final report with generate_report".to_string(),
                reason: "Compile all findings for submission".to_string(),
                done: false,
            });
        }
        _ => {}
    }

    items
}
