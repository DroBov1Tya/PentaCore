use axum::Json;
use serde_json::json;

pub async fn instructions() -> Json<serde_json::Value> {
    Json(json!({
        "name": "HackStorage_MCP",
        "version": "0.1.0",
        "description": "Persistent context store for pentest sessions. Saves endpoints, findings, requests, credentials, coverage progress and attack chains per target domain.",
        "rules": [
            "Always call GET /targets/{domain}/summary at session start to restore context",
            "Save every discovered endpoint immediately via POST /targets/{domain}/endpoints",
            "A finding is confirmed only with a working PoC — use status=potential until then",
            "Save raw HTTP request+response via POST /endpoints/{id}/requests for every interesting interaction",
            "Update coverage status after testing each vector on an endpoint",
            "Link related findings into attack chains to document kill chains"
        ],
        "endpoints": [
            {
                "method": "GET",
                "path": "/targets/{domain}/summary",
                "description": "Full picture of the target. Call this first at every session.",
                "returns": {
                    "domain": "string",
                    "endpoints_total": "int",
                    "endpoints_200": "int",
                    "findings_total": "int",
                    "critical": "int", "high": "int", "medium": "int", "low": "int",
                    "confirmed": "int", "potential": "int"
                }
            },
            {
                "method": "GET",
                "path": "/targets/{domain}/endpoints",
                "description": "List discovered endpoints.",
                "query_params": { "status": "optional int — HTTP status code filter" }
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
                    "notes": "optional string"
                }
            },
            {
                "method": "GET",
                "path": "/targets/{domain}/findings",
                "description": "List findings ordered by severity.",
                "query_params": {
                    "severity": "optional — info|low|medium|high|critical",
                    "status": "optional — potential|confirmed|false_positive"
                }
            },
            {
                "method": "POST",
                "path": "/targets/{domain}/findings",
                "description": "Save a finding. Use status=potential until PoC is ready.",
                "body": {
                    "type": "sqli|xss|ssrf|idor|rce|lfi|xxe|ssti|csrf|cors|auth|other",
                    "severity": "info|low|medium|high|critical",
                    "status": "optional — potential(default)|confirmed|false_positive",
                    "endpoint_id": "optional int",
                    "request_id": "optional int — link to saved raw request",
                    "parent_id": "optional int — link to parent finding in chain",
                    "raw_request": "optional string — exact HTTP request used",
                    "payload": "optional string",
                    "evidence": "optional string — response or proof"
                }
            },
            {
                "method": "GET",
                "path": "/endpoints/{endpoint_id}/requests",
                "description": "Timeline of all requests to this endpoint."
            },
            {
                "method": "POST",
                "path": "/endpoints/{endpoint_id}/requests",
                "description": "Save raw HTTP request and response for timeline.",
                "body": {
                    "raw_request": "string — full HTTP request with headers and body",
                    "raw_response": "optional string — full HTTP response",
                    "status_code": "optional int",
                    "response_time_ms": "optional int",
                    "notes": "optional string"
                }
            },
            {
                "method": "GET",
                "path": "/endpoints/{endpoint_id}/coverage",
                "description": "Coverage progress for this endpoint.",
                "query_params": { "status": "optional — pending|in_progress|done|skipped" }
            },
            {
                "method": "POST",
                "path": "/endpoints/{endpoint_id}/coverage",
                "description": "Update test coverage status for a vector. Upserts automatically.",
                "body": {
                    "vector": "sqli|xss|ssrf|csrf|idor|bola|rce|lfi|xxe|ssti|auth|cors|other",
                    "status": "pending|in_progress|done|skipped",
                    "notes": "optional string"
                }
            },
            {
                "method": "GET",
                "path": "/targets/{domain}/credentials",
                "description": "List saved credentials for the target."
            },
            {
                "method": "POST",
                "path": "/targets/{domain}/credentials",
                "description": "Save a credential found during testing.",
                "body": {
                    "type": "basic|cookie|token|apikey|other",
                    "username": "optional string",
                    "secret": "string — password, token or key value",
                    "notes": "optional string"
                }
            },
            {
                "method": "GET",
                "path": "/targets/{domain}/chains",
                "description": "List attack chains — documented kill chains for this target."
            },
            {
                "method": "POST",
                "path": "/targets/{domain}/chains",
                "description": "Create a new attack chain to link related findings.",
                "body": {
                    "title": "string — e.g. SSRF → Internal Discovery → RCE",
                    "description": "optional string",
                    "severity": "info|low|medium|high|critical"
                }
            },
            {
                "method": "GET",
                "path": "/chains/{chain_id}/steps",
                "description": "Get all steps of an attack chain with finding details."
            },
            {
                "method": "POST",
                "path": "/chains/{chain_id}/steps",
                "description": "Add a finding as a step in the attack chain.",
                "body": {
                    "finding_id": "int",
                    "step_order": "int — position in chain, starts from 1",
                    "notes": "optional string — what this step unlocked"
                }
            }
        ]
    }))
}
