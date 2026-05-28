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
    pub suggested_tools: Vec<ToolSuggestion>,
    /// Lessons from past engagements matching current situation (populated externally via RAG)
    pub recalled_lessons: Vec<serde_json::Value>,
}

#[derive(Serialize)]
pub struct ToolSuggestion {
    pub tool: String,
    pub purpose: String,
    pub example: String,
    pub install: String,
    pub fallback: String,
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
                suggested_tools: vec![],
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

    let suggested_tools = phase_tools(&current_phase);

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
        suggested_tools,
        recalled_lessons: vec![],
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

fn phase_tools(phase: &str) -> Vec<ToolSuggestion> {
    match phase {
        "recon" => vec![
            ToolSuggestion {
                tool: "bbot".to_string(),
                purpose: "Subdomain and infrastructure enumeration - passive and active".to_string(),
                example: "bbot -t example.com -f subdomain-enum".to_string(),
                install: "brew install bbot  OR  uv venv .venv && source .venv/bin/activate && uv pip install bbot".to_string(),
                fallback: "while read s; do host $s.example.com 2>/dev/null | grep 'has address'; done < /usr/share/seclists/Discovery/DNS/subdomains-top1million-5000.txt".to_string(),
            },
            ToolSuggestion {
                tool: "ffuf".to_string(),
                purpose: "Directory and path fuzzing - run in parallel with subdomain enum".to_string(),
                example: "ffuf -u https://example.com/FUZZ -w /usr/share/seclists/Discovery/Web-Content/raft-medium-directories.txt -mc 200,301,302,403 -t 40".to_string(),
                install: "brew install ffuf  OR  go install github.com/ffuf/ffuf/v2@latest".to_string(),
                fallback: "while read p; do code=$(curl -s -o /dev/null -w '%{http_code}' https://example.com/$p); [ \"$code\" != '404' ] && echo \"$code $p\"; done < wordlist.txt".to_string(),
            },
            ToolSuggestion {
                tool: "feroxbuster".to_string(),
                purpose: "Recursive directory bruteforce - good for apps with deep paths".to_string(),
                example: "feroxbuster -u https://example.com -w /usr/share/seclists/Discovery/Web-Content/raft-medium-words.txt --depth 3".to_string(),
                install: "brew install feroxbuster  OR  cargo install feroxbuster".to_string(),
                fallback: "Use ffuf with -recursion flag: ffuf -u https://example.com/FUZZ -w wordlist.txt -recursion -recursion-depth 3 -mc 200,301,302".to_string(),
            },
            ToolSuggestion {
                tool: "whatweb".to_string(),
                purpose: "Technology fingerprinting - stack, frameworks, versions".to_string(),
                example: "whatweb https://example.com -a 3".to_string(),
                install: "brew install whatweb  OR  apt install whatweb".to_string(),
                fallback: "curl -sI https://example.com | grep -iE 'server:|x-powered-by:|x-generator:|set-cookie:|via:'".to_string(),
            },
            ToolSuggestion {
                tool: "nuclei".to_string(),
                purpose: "Tech detection and known CVE scan in one pass".to_string(),
                example: "nuclei -u https://example.com -tags tech,cve -severity critical,high".to_string(),
                install: "brew install nuclei  OR  go install github.com/projectdiscovery/nuclei/v3/cmd/nuclei@latest".to_string(),
                fallback: "curl -s https://example.com/robots.txt; curl -s https://example.com/.git/HEAD; curl -s https://example.com/.env; curl -s https://example.com/phpinfo.php".to_string(),
            },
            ToolSuggestion {
                tool: "git / gix".to_string(),
                purpose: "Source history analysis - security commits, regressions, removed checks. gix is faster on large repos".to_string(),
                example: "git log --all --oneline | grep -iE 'fix|security|CVE|auth|bypass|vuln|remove|revert'  OR  gix log --all | grep -iE 'fix|security|auth'".to_string(),
                install: "brew install git gitoxide  OR  cargo install gitoxide".to_string(),
                fallback: "git is almost always pre-installed. If gix is unavailable: git log --all -p --follow -- path/to/file | grep '^[-+]' covers most diff analysis needs".to_string(),
            },
            ToolSuggestion {
                tool: "gau / waybackurls".to_string(),
                purpose: "Historical URLs from web archives - surfaces hidden endpoints and old params".to_string(),
                example: "gau example.com | tee urls.txt && cat urls.txt | grep -E '\\?|&' | sort -u".to_string(),
                install: "brew install gau  OR  go install github.com/lc/gau/v2/cmd/gau@latest".to_string(),
                fallback: "curl -s 'https://web.archive.org/cdx/search/cdx?url=example.com/*&output=text&fl=original&collapse=urlkey' | sort -u | grep -E '\\?|&'".to_string(),
            },
            ToolSuggestion {
                tool: "rustscan".to_string(),
                purpose: "Fast port scan, then pipes open ports into nmap for service detection".to_string(),
                example: "rustscan -a example.com -- -sV -sC".to_string(),
                install: "cargo install rustscan  OR  brew install rustscan".to_string(),
                fallback: "nmap -p- --min-rate 5000 -T4 example.com  OR  for p in 80 443 8080 8443 3000 5000; do (echo > /dev/tcp/example.com/$p) 2>/dev/null && echo \"$p open\"; done".to_string(),
            },
            ToolSuggestion {
                tool: "resolve_dns (built-in)".to_string(),
                purpose: "DNS records: A, MX, TXT, NS".to_string(),
                example: "resolve_dns(domain: example.com)".to_string(),
                install: "built into PentaCore - no install needed".to_string(),
                fallback: "dig example.com ANY +noall +answer; dig TXT example.com; dig MX example.com".to_string(),
            },
            ToolSuggestion {
                tool: "enumerate_subdomains (built-in)".to_string(),
                purpose: "Fast common subdomain resolution".to_string(),
                example: "enumerate_subdomains(domain: example.com)".to_string(),
                install: "built into PentaCore - no install needed".to_string(),
                fallback: "for s in www api dev staging admin mail; do dig +short $s.example.com; done".to_string(),
            },
        ],
        "enumeration" => vec![
            ToolSuggestion {
                tool: "ffuf".to_string(),
                purpose: "Parameter and endpoint fuzzing with filter tuning".to_string(),
                example: "ffuf -u https://example.com/FUZZ -w /usr/share/seclists/Discovery/Web-Content/raft-medium-directories.txt -mc 200,301,302,403".to_string(),
                install: "brew install ffuf  OR  go install github.com/ffuf/ffuf/v2@latest".to_string(),
                fallback: "while read p; do code=$(curl -sk -o /dev/null -w '%{http_code}' https://example.com/$p); [ \"$code\" != '404' ] && echo \"$code $p\"; done < wordlist.txt".to_string(),
            },
            ToolSuggestion {
                tool: "x8".to_string(),
                purpose: "Hidden parameter discovery - finds query/body params the app accepts but doesn't document".to_string(),
                example: "x8 -u https://example.com/api/user -w /usr/share/seclists/Discovery/Web-Content/burp-parameter-names.txt".to_string(),
                install: "cargo install x8  OR  brew install x8".to_string(),
                fallback: "ffuf -u 'https://example.com/api/user?FUZZ=test' -w params.txt -fs <baseline-size> (filter by response size to spot accepted params)".to_string(),
            },
            ToolSuggestion {
                tool: "parse_api_spec (built-in)".to_string(),
                purpose: "Import all routes from OpenAPI/Swagger spec automatically".to_string(),
                example: "parse_api_spec(domain: example.com, url: https://example.com/swagger.json)".to_string(),
                install: "built into PentaCore - no install needed".to_string(),
                fallback: "curl -s https://example.com/swagger.json | python3 -c \"import sys,json; [print(m.upper(), p) for p,v in json.load(sys.stdin)['paths'].items() for m in v]\"".to_string(),
            },
            ToolSuggestion {
                tool: "parse_graphql_spec (built-in)".to_string(),
                purpose: "Import GraphQL schema via introspection".to_string(),
                example: "parse_graphql_spec(domain: example.com, url: https://example.com/graphql)".to_string(),
                install: "built into PentaCore - no install needed".to_string(),
                fallback: "curl -s -X POST https://example.com/graphql -H 'Content-Type: application/json' -d '{\"query\":\"{__schema{types{name fields{name}}}}\"}' | python3 -m json.tool".to_string(),
            },
            ToolSuggestion {
                tool: "burp suite".to_string(),
                purpose: "Passive crawl and JS analysis for hidden endpoints".to_string(),
                example: "Proxy browser traffic, use target > site map, spider all in scope".to_string(),
                install: "brew install --cask burp-suite  OR  https://portswigger.net/burp/communitydownload".to_string(),
                fallback: "curl -s https://example.com | grep -oE '(href|src|action)=\"[^\"]+\"' | sed 's/.*=\"//;s/\"//' | sort -u".to_string(),
            },
        ],
        "vuln_mapping" => vec![
            ToolSuggestion {
                tool: "nuclei".to_string(),
                purpose: "Automated vulnerability templates across the full surface".to_string(),
                example: "nuclei -u https://example.com -tags sqli,xss,ssrf,lfi,rce -severity medium,high,critical".to_string(),
                install: "brew install nuclei  OR  go install github.com/projectdiscovery/nuclei/v3/cmd/nuclei@latest".to_string(),
                fallback: "Run manual checks per vector: curl with SQL metacharacters, XSS payloads, path traversal sequences - one parameter at a time".to_string(),
            },
            ToolSuggestion {
                tool: "sqlmap".to_string(),
                purpose: "SQL injection detection on specific endpoints".to_string(),
                example: "sqlmap -u 'https://example.com/api/search?q=test' --level=3 --risk=2 --batch".to_string(),
                install: "brew install sqlmap  OR  uv venv .venv && uv pip install sqlmap".to_string(),
                fallback: "curl 'https://example.com/api/search?q=test%27' -s | grep -iE 'sql|syntax|mysql|pg|ora|error'  -- then try: q=1 AND 1=1-- and q=1 AND 1=2--".to_string(),
            },
            ToolSuggestion {
                tool: "dalfox".to_string(),
                purpose: "XSS scanning with parameter analysis".to_string(),
                example: "dalfox url 'https://example.com/search?q=test' --follow-redirects".to_string(),
                install: "brew install dalfox  OR  go install github.com/hahwul/dalfox/v2@latest".to_string(),
                fallback: "curl -s 'https://example.com/search?q=<script>alert(1)</script>' | grep -o '<script>alert(1)</script>'  -- also try: q=\"><img src=x onerror=alert(1)>".to_string(),
            },
            ToolSuggestion {
                tool: "jwt_tool".to_string(),
                purpose: "JWT attack automation: alg:none, RS256->HS256, kid injection".to_string(),
                example: "python3 jwt_tool.py <token> -M at".to_string(),
                install: "git clone https://github.com/ticarpi/jwt_tool && uv venv .venv && source .venv/bin/activate && uv pip install -r requirements.txt".to_string(),
                fallback: "python3 -c \"import base64,json; parts=input().split('.'); h=json.loads(base64.b64decode(parts[0]+'==')); h['alg']='none'; import base64 as b; print(b.urlsafe_b64encode(json.dumps(h).encode()).rstrip(b'=')+b'.'+parts[1].encode()+b'.')\"".to_string(),
            },
            ToolSuggestion {
                tool: "make_race_requests (built-in)".to_string(),
                purpose: "Concurrent requests for race condition and limit bypass testing".to_string(),
                example: "make_race_requests(method: POST, url: /api/transfer, count: 20, threads: 10)".to_string(),
                install: "built into PentaCore - no install needed".to_string(),
                fallback: "for i in $(seq 1 20); do curl -s -X POST https://example.com/api/transfer -H 'Authorization: Bearer TOKEN' -d '{\"amount\":4999}' & done; wait".to_string(),
            },
            ToolSuggestion {
                tool: "replay_as (built-in)".to_string(),
                purpose: "IDOR testing: replay saved request with a different user session".to_string(),
                example: "replay_as(request_id: 42, cookies: [session=other_user_cookie])".to_string(),
                install: "built into PentaCore - no install needed".to_string(),
                fallback: "curl -s https://example.com/api/orders/1337 -H 'Cookie: session=OTHER_USER_SESSION' -- compare response with your own session".to_string(),
            },
        ],
        "exploitation" => vec![
            ToolSuggestion {
                tool: "make_request (built-in)".to_string(),
                purpose: "Send crafted HTTP requests with full session context and auto-save evidence".to_string(),
                example: "make_request(method: POST, url: /api/admin, body: ..., endpoint_id: 5)".to_string(),
                install: "built into PentaCore - no install needed".to_string(),
                fallback: "curl -s -X POST https://example.com/api/admin -H 'Content-Type: application/json' -H 'Authorization: Bearer TOKEN' -d '{...}' -v".to_string(),
            },
            ToolSuggestion {
                tool: "diff_requests (built-in)".to_string(),
                purpose: "Compare two saved responses to confirm IDOR or blind injection".to_string(),
                example: "diff_requests(request_id_a: 10, request_id_b: 11)".to_string(),
                install: "built into PentaCore - no install needed".to_string(),
                fallback: "curl -s URL1 > /tmp/r1.txt && curl -s URL2 > /tmp/r2.txt && diff /tmp/r1.txt /tmp/r2.txt".to_string(),
            },
            ToolSuggestion {
                tool: "interactsh".to_string(),
                purpose: "Out-of-band detection for blind SSRF, XXE, blind RCE".to_string(),
                example: "interactsh-client -v  -- use generated URL in SSRF/XXE payloads".to_string(),
                install: "brew install interactsh  OR  go install github.com/projectdiscovery/interactsh/cmd/interactsh-client@latest".to_string(),
                fallback: "Use Burp Collaborator (free tier) or requestbin.com/r -- paste the unique URL into SSRF parameters and watch for DNS/HTTP callbacks".to_string(),
            },
            ToolSuggestion {
                tool: "burp suite".to_string(),
                purpose: "Manual PoC crafting, repeater, intruder for credential brute-force".to_string(),
                example: "Send suspicious request to Repeater, modify JWT claims or inject payloads manually".to_string(),
                install: "brew install --cask burp-suite  OR  https://portswigger.net/burp/communitydownload".to_string(),
                fallback: "curl -v with manually crafted headers and body covers 90% of repeater use cases".to_string(),
            },
        ],
        "post_exploitation" => vec![
            ToolSuggestion {
                tool: "ffuf".to_string(),
                purpose: "Internal network and path discovery via SSRF pivot".to_string(),
                example: "ffuf -u 'https://example.com/fetch?url=http://FUZZ/' -w internal-hosts.txt -fw 10".to_string(),
                install: "brew install ffuf  OR  go install github.com/ffuf/ffuf/v2@latest".to_string(),
                fallback: "for ip in 10.0.0.{1..254}; do curl -s 'https://example.com/fetch?url=http://'$ip'/' --max-time 2 | grep -v 'Connection refused' && echo $ip; done".to_string(),
            },
            ToolSuggestion {
                tool: "aws cli".to_string(),
                purpose: "Enumerate IAM permissions and cloud resources with stolen IMDS credentials".to_string(),
                example: "AWS_ACCESS_KEY_ID=... AWS_SECRET_ACCESS_KEY=... AWS_SESSION_TOKEN=... aws sts get-caller-identity".to_string(),
                install: "brew install awscli  OR  uv venv .venv && uv pip install awscli".to_string(),
                fallback: "curl -s http://169.254.169.254/latest/meta-data/iam/security-credentials/ -- then use the role name to fetch keys, then call AWS API with curl and SigV4 signing".to_string(),
            },
            ToolSuggestion {
                tool: "linpeas".to_string(),
                purpose: "Privilege escalation enumeration if shell access obtained".to_string(),
                example: "curl -sL https://github.com/peass-ng/PEASS-ng/releases/latest/download/linpeas.sh | sh".to_string(),
                install: "no install - runs as shell script directly".to_string(),
                fallback: "id; sudo -l; find / -perm -4000 2>/dev/null; cat /etc/crontab; env; ls -la /home".to_string(),
            },
        ],
        "reporting" => vec![
            ToolSuggestion {
                tool: "generate_report (built-in)".to_string(),
                purpose: "Compile all findings, coverage, and evidence into a structured report".to_string(),
                example: "generate_report(domain: example.com)".to_string(),
                install: "built into PentaCore - no install needed".to_string(),
                fallback: "recall_engagement_state(lens: progress) + get_findings + export findings manually to markdown".to_string(),
            },
        ],
        _ => vec![],
    }
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
