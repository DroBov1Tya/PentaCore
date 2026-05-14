use axum::Json;
use serde_json::json;

use crate::constants::{VERSION, VERSION_DATE};

pub async fn instructions() -> Json<serde_json::Value> {
    Json(json!({
        "name": "pentest-context-mcp",
        "version": VERSION,
        "description": "Persistent context store for pentest sessions. Saves tokens by giving structured, queryable memory across sessions. One targeted request returns exactly what you need instead of re-reading files or reconstructing state.",
        "how_to_use": {
            "session_start": [
                "1. GET /targets/{domain}/scope — read engagement rules before any action",
                "2. GET /targets/{domain}/summary — restore full context: endpoints found, findings, confirmed count",
                "3. GET /targets/{domain}/relations — check domain links, assess pivot potential",
                "4. GET /endpoints/{id}/coverage?status=pending — find untested vectors, continue from where you stopped"
            ],
            "during_work": [
                "Save every endpoint immediately — do not accumulate in memory",
                "Save every interesting request/response via POST /endpoints/{id}/requests with raw_request and raw_response",
                "Update coverage after testing each vector — this is your progress tracker",
                "Create finding on first suspicion with status=potential, upgrade to confirmed only with working PoC",
                "Link related findings via attack_chains — this becomes the kill chain section of your report"
            ],
            "why_it_helps": "LLM sessions have limited context. Without this MCP you lose history on restart, waste tokens re-reading files, and cannot see vector coverage gaps. With MCP — one request returns the exact data slice needed right now."
        },
        "rules": [
            "ALWAYS start session with GET /targets/{domain}/scope — never act without knowing engagement rules",
            "A finding is confirmed only with a reproducible PoC — use status=potential until then",
            "Save raw request and response for every finding — this is your evidence base",
            "No findings means incomplete coverage, not a clean target",
            "Check coverage before closing a phase — gaps visible via status=pending"
        ],
        "endpoints": [
            {
                "method": "GET",
                "path": "/targets/{domain}/scope",
                "description": "Engagement rules: objective, what is in scope, what is forbidden, constraints. Call first every session.",
                "returns": {
                    "objective": "End goal — e.g. find RCE",
                    "in_scope": "What is allowed to test",
                    "out_of_scope": "What must not be touched",
                    "rules": "Extra rules — rate limit, no destructive actions, etc."
                }
            },
            {
                "method": "POST",
                "path": "/targets/{domain}/scope",
                "description": "Set or update scope for a target.",
                "body": {
                    "objective": "string",
                    "in_scope": "string",
                    "out_of_scope": "optional string",
                    "rules": "optional string"
                }
            },
            {
                "method": "GET",
                "path": "/targets/{domain}/summary",
                "description": "Full target picture in one request. Restores session context without reading files.",
                "returns": {
                    "domain": "string",
                    "endpoints_total": "int",
                    "endpoints_200": "int",
                    "findings_total": "int",
                    "critical": "int",
                    "high": "int",
                    "medium": "int",
                    "low": "int",
                    "confirmed": "int",
                    "potential": "int"
                }
            },
            {
                "method": "GET",
                "path": "/targets/{domain}/relations",
                "description": "Domain relationships. Shows subdomains, shared infra, pivot points.",
                "returns": "array — from_domain, to_domain, rel_type, description"
            },
            {
                "method": "POST",
                "path": "/targets/{domain}/relations",
                "description": "Save a domain relationship found during recon.",
                "body": {
                    "to_domain": "string",
                    "rel_type": "subdomain | cdn | shared_infra | pivot | related",
                    "description": "optional string — how discovered, what it unlocks"
                }
            },
            {
                "method": "GET",
                "path": "/targets/{domain}/endpoints",
                "description": "All discovered endpoints. Use status filter to avoid over-fetching.",
                "query_params": {
                    "status": "optional int — HTTP status filter: 200, 403, 500, etc."
                }
            },
            {
                "method": "POST",
                "path": "/targets/{domain}/endpoints",
                "description": "Save a discovered endpoint. Target auto-created if not exists.",
                "body": {
                    "method": "GET | POST | PUT | DELETE | PATCH",
                    "path": "string — e.g. /api/v1/users",
                    "status_code": "optional int",
                    "auth": "optional bool",
                    "description": "optional string — what the endpoint does, what data it returns",
                    "notes": "optional string — observations, anomalies"
                }
            },
            {
                "method": "GET",
                "path": "/targets/{domain}/findings",
                "description": "Findings sorted by severity. Filter to get the exact slice you need.",
                "query_params": {
                    "severity": "optional — info | low | medium | high | critical",
                    "status": "optional — potential | confirmed | false_positive"
                }
            },
            {
                "method": "POST",
                "path": "/targets/{domain}/findings",
                "description": "Save a finding. Create on first suspicion, update status as you verify.",
                "body": {
                    "type": "sqli | xss | ssrf | idor | rce | lfi | xxe | ssti | csrf | cors | auth | other",
                    "severity": "info | low | medium | high | critical",
                    "status": "optional — potential(default) | confirmed | false_positive",
                    "endpoint_id": "optional int",
                    "request_id": "optional int — link to saved raw request",
                    "parent_id": "optional int — parent finding if this is a consequence",
                    "raw_request": "optional string — exact HTTP request that reproduces the vuln",
                    "payload": "optional string",
                    "evidence": "optional string — server response as proof",
                    "description": "optional string — vuln description, impact, how to exploit"
                }
            },
            {
                "method": "GET",
                "path": "/endpoints/{endpoint_id}/requests",
                "description": "Timeline of all requests to this endpoint. Use to compare behavior across payloads and reconstruct test history."
            },
            {
                "method": "POST",
                "path": "/endpoints/{endpoint_id}/requests",
                "description": "Save raw HTTP request and response. Evidence base and behavior change history.",
                "body": {
                    "raw_request": "string — full HTTP request with headers and body",
                    "raw_response": "optional string — full HTTP response",
                    "status_code": "optional int",
                    "response_time_ms": "optional int — abnormal timing may indicate blind injection",
                    "description": "optional string — why this request was sent, what was tested",
                    "notes": "optional string — observations in response"
                }
            },
            {
                "method": "GET",
                "path": "/endpoints/{endpoint_id}/coverage",
                "description": "Vector testing progress for this endpoint. Call with status=pending to find untested vectors.",
                "query_params": {
                    "status": "optional — pending | in_progress | done | skipped"
                }
            },
            {
                "method": "POST",
                "path": "/endpoints/{endpoint_id}/coverage",
                "description": "Update vector test status. Upsert — safe to call repeatedly.",
                "body": {
                    "vector": "sqli | xss | ssrf | csrf | idor | bola | rce | lfi | xxe | ssti | auth | cors | other",
                    "status": "pending | in_progress | done | skipped",
                    "description": "optional string — test result or reason for skip",
                    "notes": "optional string"
                }
            },
            {
                "method": "GET",
                "path": "/targets/{domain}/credentials",
                "description": "Saved credentials. Use to attempt access to protected endpoints."
            },
            {
                "method": "POST",
                "path": "/targets/{domain}/credentials",
                "description": "Save a discovered credential.",
                "body": {
                    "type": "basic | cookie | token | apikey | other",
                    "username": "optional string",
                    "secret": "string — password, token or key",
                    "description": "optional string — where found, what it grants access to",
                    "notes": "optional string"
                }
            },
            {
                "method": "GET",
                "path": "/targets/{domain}/chains",
                "description": "Attack chains — documented kill chains showing how findings form an attack sequence."
            },
            {
                "method": "POST",
                "path": "/targets/{domain}/chains",
                "description": "Create an attack chain when multiple findings form a sequence.",
                "body": {
                    "title": "string — e.g. SSRF → Internal Discovery → RCE",
                    "description": "optional string — chain description and impact",
                    "severity": "info | low | medium | high | critical"
                }
            },
            {
                "method": "GET",
                "path": "/chains/{chain_id}/steps",
                "description": "All chain steps with finding details. Ready structure for kill chain section in report."
            },
            {
                "method": "POST",
                "path": "/chains/{chain_id}/steps",
                "description": "Add a finding as a step in the chain.",
                "body": {
                    "finding_id": "int",
                    "step_order": "int — position starting from 1",
                    "notes": "optional string — what this step unlocked for the next"
                }
            }
        ]
    }))
}
