use serde_json::{Value, json};

const SUPPORTED_PROTOCOL_VERSIONS: &[&str] =
    &["2025-11-25", "2025-06-18", "2025-03-26", "2024-11-05"];

pub fn initialize_msg(id: &Value, requested_version: &str) -> Value {
    let version_to_use = if SUPPORTED_PROTOCOL_VERSIONS.contains(&requested_version) {
        requested_version
    } else {
        "2024-11-05" // fallback
    };

    json!({
        "jsonrpc": "2.0",
        "id": id,
        "result": {
            "protocolVersion": version_to_use,
            "capabilities": {
                "tools": {},
                "resources": {}
            },
            "serverInfo": {
                "name": "PentaCore-mcp-stdio",
                "version": crate::constants::VERSION
            },
            "instructions": "Read `pentest://instructions` at session start.\n\nThen always run in order:\n1. `get_scope(domain)`\n2. `get_phase_playbook(domain)`\n3. `search_knowledge(<scenario_query>, domain: \"global\")`\n\nIf `get_scope` returns not found: call `save_scope` first."
        }
    })
}

pub fn resources_list_msg(id: &Value) -> Value {
    json!({
        "jsonrpc": "2.0",
        "id": id,
        "result": {
            "resources": [
                {
                    "uri": "pentest://instructions",
                    "name": "Pentest Context MCP Instructions",
                    "description": "Rules and manual for using PentaCore. Read this to understand how to store context properly."
                }
            ]
        }
    })
}

pub fn resources_read_msg(id: &Value, server_path: &str) -> Value {
    json!({
        "jsonrpc": "2.0",
        "id": id,
        "result": {
            "contents": [{
                "uri": "pentest://instructions",
                "mimeType": "text/markdown",
                "text": format!("## PentaCore MCP — Security Research Memory Server
REST API also available at localhost:{} (same DB).

### OODA Loop
1. **Observe** — `recall_engagement_state(domain, lens)`
2. **Orient** — `search_knowledge` for techniques; state assumptions (EXPECTED vs ENFORCED)
3. **Decide** — pick assumption most likely wrong; check `recall_similar_situations`
4. **Act** — test it; know what vulnerable vs safe looks like in advance
5. **Reflect** — `record_lesson`; `save_dead_end` if ruled out

When you confirm a finding: ask 'where else does the same root cause exist?' Save follow-up hypotheses.

### Knowledge Base
Query BEFORE starting analysis. Categories: mindset, technique (auth/JWT/OAuth/SSRF/IDOR/race/GraphQL/cloud/mobile/binary), methodology, tools.

domain: pass target domain for engagement-specific, omit or `global` for shared KB.

Routing table:
| Target | search_knowledge query |
|--------|----------------------|
| Web (no source) | web surface mapping IDOR role matrix business logic |
| Web + source | git history auth flow taint analysis sinks |
| Source only | static analysis grep secrets sinks code audit |
| Binary | memory safety buffer integer overflow control flow |
| Mobile APK/IPA | android ios dynamic instrumentation ssl pinning deeplink |
| Kubernetes/cloud | rbac iam privilege container isolation imds |
| Docker image | container layers secrets entrypoint |
| Network/AD | active directory smb kerberos lateral movement |

Extra searches for web/API: `API mass assignment BOLA BFLA rate limit` and `business logic workflow race coupon quota`.

Add knowledge: `memorize_concept(domain: global, category: technique|mindset|methodology)`.

### Rules
- Session start: get_scope → get_phase_playbook → search_knowledge
- `save_scope` first if get_scope returns not found
- status=potential until reproducible evidence; then update_finding to confirmed
- save_dead_end when a technique fails — prevents re-testing
- save raw request+response for every finding

### Agents
track_agent → launch sub-agent with ID → sub-agent calls update_agent_status when done → orchestrator reads list_agents + recall_engagement_state.

### HTTP Client
- set_session: global cookies/auth for make_request
- make_request: omit user_agent to randomize
- revoke_session on 401/403
- make_race_requests for concurrency/TOCTOU testing", server_path.split(':').last().unwrap_or("8082"))
            }]
        }
    })
}

pub fn tools_list_msg(id: &Value) -> Value {
    json!({
        "jsonrpc": "2.0",
        "id": id,
        "result": {
            "tools": [
                {
                    "name": "get_scope",
                    "description": "Get engagement rules and scope for a target.",
                    "inputSchema": { "type": "object", "properties": { "domain": { "type": "string" } }, "required": ["domain"] }
                },
                {
                    "name": "save_scope",
                    "description": "Save or update engagement scope and rules. Include domain_type to filter recalled lessons by engagement type.",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "domain": { "type": "string" },
                            "objective": { "type": "string" },
                            "in_scope": { "type": "string" },
                            "out_of_scope": { "type": "string" },
                            "rules": { "type": "string" },
                            "domain_type": { "type": "string", "enum": ["web","binary","cloud","infra","mobile"], "description": "Type of engagement - filters recalled lessons to only show relevant past experience" }
                        },
                        "required": ["domain", "objective", "in_scope"]
                    }
                },
                {
                    "name": "get_relations",
                    "description": "Get domain relationships (subdomains, pivots).",
                    "inputSchema": { "type": "object", "properties": { "domain": { "type": "string" } }, "required": ["domain"] }
                },
                {
                    "name": "save_relation",
                    "description": "Save domain relationship.",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "from_domain": { "type": "string" }, "to_domain": { "type": "string" },
                            "rel_type": { "type": "string", "enum": ["subdomain", "cdn", "shared_infra", "pivot", "related"] },
                            "description": { "type": "string" }
                        },
                        "required": ["from_domain", "to_domain", "rel_type"]
                    }
                },
                {
                    "name": "memorize_concept",
                    "description": "Store a concept into the RAG knowledge base. category: technique|mindset|methodology. domain='global' for cross-engagement knowledge.",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "domain": { "type": "string", "description": "The target domain this memory belongs to" },
                            "category": { "type": "string", "description": "Category of the memory (e.g., 'auth_bypass_idea', 'raw_docs', 'graphql_schema')" },
                            "title": { "type": "string", "description": "Short, descriptive title" },
                            "content": { "type": "string", "description": "The actual text/data to memorize" },
                            "tags": { "type": "array", "items": { "type": "string" }, "description": "Array of tags" }
                        },
                        "required": ["domain", "category", "title", "content"]
                    }
                },
                {
                    "name": "search_knowledge",
                    "description": "Semantic search over the knowledge base (techniques, mindset, methodology) and engagement memories. Call before analysis or approaching unfamiliar surfaces.",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "query": { "type": "string", "description": "The natural language or code query to search for" },
                            "domain": { "type": "string", "description": "Optional domain to filter results" },
                            "limit": { "type": "integer", "description": "Max number of results to return (default 5)" }
                        },
                        "required": ["query"]
                    }
                },
                {
                    "name": "list_memories",
                    "description": "List stored memories without semantic search. Use this to browse what has been saved.",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "domain": { "type": "string", "description": "Optional domain to filter results" },
                            "limit": { "type": "integer", "description": "Max number of results to return (default 10)" }
                        }
                    }
                },
                {
                    "name": "forget_memory",
                    "description": "Delete a memory note by ID.",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "id": { "type": "string", "description": "The UUID string of the memory to delete" }
                        },
                        "required": ["id"]
                    }
                },
                {
                    "name": "get_memory",
                    "description": "Retrieve a single memory by ID.",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "id": { "type": "string", "description": "The UUID string of the memory" }
                        },
                        "required": ["id"]
                    }
                },
                {
                    "name": "update_memory",
                    "description": "Update an existing memory by ID. Only provided fields are updated.",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "id": { "type": "string", "description": "The UUID of the memory to update" },
                            "category": { "type": "string" },
                            "title": { "type": "string" },
                            "content": { "type": "string" },
                            "tags": { "type": "array", "items": { "type": "string" } }
                        },
                        "required": ["id"]
                    }
                },
                {
                    "name": "get_endpoints",
                    "description": "List discovered endpoints.",
                    "inputSchema": {
                        "type": "object", "properties": { "domain": { "type": "string" }, "status": { "type": "integer" } }, "required": ["domain"]
                    }
                },
                {
                    "name": "save_endpoint",
                    "description": "Save an endpoint discovered during recon.",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "domain": { "type": "string" }, "method": { "type": "string" }, "path": { "type": "string" },
                            "status_code": { "type": "integer" }, "auth": { "type": "boolean" },
                            "description": { "type": "string" }, "notes": { "type": "string" }
                        },
                        "required": ["domain", "method", "path"]
                    }
                },
                {
                    "name": "get_findings",
                    "description": "List vulnerability findings.",
                    "inputSchema": {
                        "type": "object",
                        "properties": { "domain": { "type": "string" }, "severity": { "type": "string" }, "status": { "type": "string" } },
                        "required": ["domain"]
                    }
                },
                {
                    "name": "save_finding",
                    "description": "Save a vulnerability finding.",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "domain": { "type": "string" }, "type": { "type": "string" },
                            "severity": { "type": "string", "enum": ["info", "low", "medium", "high", "critical"] },
                            "status": { "type": "string", "enum": ["potential", "confirmed", "false_positive"] },
                            "endpoint_id": { "type": "integer" }, "request_id": { "type": "integer" },
                            "description": { "type": "string" }, "payload": { "type": "string" }, "evidence": { "type": "string" }
                        },
                        "required": ["domain", "type", "severity"]
                    }
                },
                {
                    "name": "update_finding",
                    "description": "Update an existing vulnerability finding. Use this to change status (e.g. potential -> confirmed/false_positive), update severity, or add evidence.",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "id": { "type": "integer", "description": "The ID of the finding to update" },
                            "severity": { "type": "string", "enum": ["info", "low", "medium", "high", "critical"] },
                            "status": { "type": "string", "enum": ["potential", "confirmed", "false_positive"] },
                            "evidence": { "type": "string" },
                            "description": { "type": "string" }
                        },
                        "required": ["id"]
                    }
                },
                {
                    "name": "get_coverage",
                    "description": "Get test coverage for an endpoint.",
                    "inputSchema": { "type": "object", "properties": { "endpoint_id": { "type": "integer" }, "status": { "type": "string" } }, "required": ["endpoint_id"] }
                },
                {
                    "name": "upsert_coverage",
                    "description": "Update vector test status for an endpoint.",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "endpoint_id": { "type": "integer" }, "vector": { "type": "string", "enum": ["sqli", "xss", "ssrf", "csrf", "idor", "bola", "rce", "lfi", "xxe", "ssti", "auth", "cors", "other"] },
                            "status": { "type": "string", "enum": ["pending", "in_progress", "done", "skipped"] },
                            "description": { "type": "string" }, "notes": { "type": "string" }
                        },
                        "required": ["endpoint_id", "vector", "status"]
                    }
                },
                {
                    "name": "save_request",
                    "description": "Save raw HTTP request/response evidence.",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "endpoint_id": { "type": "integer" }, "raw_request": { "type": "string" }, "raw_response": { "type": "string" },
                            "status_code": { "type": "integer" }, "description": { "type": "string" }
                        },
                        "required": ["endpoint_id", "raw_request"]
                    }
                },
                {
                    "name": "get_requests",
                    "description": "List saved requests for an endpoint.",
                    "inputSchema": { "type": "object", "properties": { "endpoint_id": { "type": "integer" } }, "required": ["endpoint_id"] }
                },
                {
                    "name": "save_credential",
                    "description": "Save discovered credential.",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "domain": { "type": "string" }, "type": { "type": "string" },
                            "username": { "type": "string" }, "secret": { "type": "string" },
                            "description": { "type": "string" }
                        },
                        "required": ["domain", "type", "secret"]
                    }
                },
                {
                    "name": "get_proxies",
                    "description": "List proxies for a target.",
                    "inputSchema": { "type": "object", "properties": { "domain": { "type": "string" } }, "required": ["domain"] }
                },
                {
                    "name": "save_proxy",
                    "description": "Save a proxy.",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "domain": { "type": "string" }, "url": { "type": "string" },
                            "type": { "type": "string", "enum": ["http", "socks5", "burp"] },
                            "active": { "type": "integer" }, "description": { "type": "string" }, "notes": { "type": "string" }
                        },
                        "required": ["domain", "url", "type"]
                    }
                },
                {
                    "name": "get_credentials",
                    "description": "List credentials.",
                    "inputSchema": { "type": "object", "properties": { "domain": { "type": "string" } }, "required": ["domain"] }
                },
                {
                    "name": "get_chains",
                    "description": "List attack chains.",
                    "inputSchema": { "type": "object", "properties": { "domain": { "type": "string" } }, "required": ["domain"] }
                },
                {
                    "name": "save_chain",
                    "description": "Create attack chain.",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "domain": { "type": "string" }, "title": { "type": "string" },
                            "severity": { "type": "string" }, "description": { "type": "string" }
                        },
                        "required": ["domain", "title", "severity"]
                    }
                },
                {
                    "name": "get_chain_steps",
                    "description": "List steps in an attack chain.",
                    "inputSchema": { "type": "object", "properties": { "chain_id": { "type": "integer" } }, "required": ["chain_id"] }
                },
                {
                    "name": "add_chain_step",
                    "description": "Add a finding to a chain.",
                    "inputSchema": {
                        "type": "object",
                        "properties": { "chain_id": { "type": "integer" }, "finding_id": { "type": "integer" }, "step_order": { "type": "integer" }, "notes": { "type": "string" } },
                        "required": ["chain_id", "finding_id", "step_order"]
                    }
                },
                {
                    "name": "set_session",
                    "description": "Set global session context (cookies, auth token) for subsequent make_request calls.",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "cookies": { "type": "array", "items": { "type": "string" } },
                            "auth_token": { "type": "string" }
                        }
                    }
                },
                {
                    "name": "enumerate_subdomains",
                    "description": "Quickly resolve common subdomains for a target to discover hidden infrastructure. Automatically saves discoveries to target_relations.",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "domain": { "type": "string" }
                        },
                        "required": ["domain"]
                    }
                },
                {
                    "name": "resolve_dns",
                    "description": "Perform standard public DNS resolution (like dig or nslookup). Returns A, AAAA, MX, TXT, and NS records for a domain.",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "domain": { "type": "string" }
                        },
                        "required": ["domain"]
                    }
                },
                {
                    "name": "revoke_session",
                    "description": "Clear the global session context (cookies, auth token). Use this when the session is expired or invalid.",
                    "inputSchema": {
                        "type": "object",
                        "properties": {}
                    }
                },
                {
                    "name": "make_request",
                    "description": "Make an HTTP request using global session context. Returns a JSON string with 'status', 'headers', 'hint' (recon or auth hints), and 'body' fields. Supports custom HTTP versions, schemas, and overriding headers.",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "method": { "type": "string" },
                            "url": { "type": "string" },
                            "body": { "type": "string" },
                            "cookies": { "type": "array", "items": { "type": "string" } },
                            "proxy": { "type": "string" },
                            "user_agent": { "type": "string", "description": "Optional custom User-Agent. If omitted, a random browser UA is used." },
                            "http_version": { "type": "string", "description": "Optional HTTP version (e.g. '1.0', '1.1', '2.0'). Defaults to '1.1'." },
                            "scheme": { "type": "string", "description": "Optional URL scheme (e.g. 'http', 'https', 'gopher', 'file'). If non-http, curl is used natively." },
                            "custom_headers": { "type": "object", "description": "Optional dictionary of custom headers to include (e.g., {'Authorization': 'NTLM TlR...', 'X-Forwarded-For': '127.0.0.1'})." },
                            "endpoint_id": { "type": "integer", "description": "Optional ID to link the request evidence to." }
                        },
                        "required": ["method", "url"]
                    }
                },
                {
                    "name": "make_race_requests",
                    "description": "Send multiple identical HTTP requests concurrently to test for race conditions (TOCTOU). HINT: It is highly recommended to use the 'proxy' parameter to avoid IP bans during aggressive testing.",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "method": { "type": "string" },
                            "url": { "type": "string" },
                            "body": { "type": "string" },
                            "count": { "type": "integer", "description": "Total number of requests to send (default 5, max 100)." },
                            "threads": { "type": "integer", "description": "Number of concurrent threads to use (default 5, max 20)." },
                            "cookies": { "type": "array", "items": { "type": "string" } },
                            "user_agent": { "type": "string", "description": "Optional custom User-Agent." },
                            "proxy": { "type": "string", "description": "Optional proxy URL." }
                        },
                        "required": ["method", "url"]
                    }
                },
                {
                    "name": "diff_requests",
                    "description": "Compare two saved HTTP responses by status code, body size, timing, and JSON structure. Use this for IDOR detection and blind injection analysis.",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "request_id_a": { "type": "integer", "description": "ID of the first request to compare." },
                            "request_id_b": { "type": "integer", "description": "ID of the second request to compare." }
                        },
                        "required": ["request_id_a", "request_id_b"]
                    }
                },
                {
                    "name": "replay_as",
                    "description": "Replay a previously saved HTTP request but with a different set of cookies/headers to test for IDOR and authorization flaws. This automatically fetches the original request by ID, strips its auth tokens, injects the new ones, and sends it.",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "request_id": { "type": "integer", "description": "ID of the saved request to replay." },
                            "cookies": { "type": "array", "items": { "type": "string" }, "description": "Array of cookie strings (e.g., 'session=XYZ') to inject." },
                            "auth_token": { "type": "string", "description": "Optional Bearer token to inject." }
                        },
                        "required": ["request_id"]
                    }
                },
                {
                    "name": "claim_test_object",
                    "description": "Register a test artifact (user, post, token, etc.) created during testing. Provide rollback_method/url/body so it can be automatically cleaned up later.",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "domain": { "type": "string" },
                            "object_type": { "type": "string", "description": "Type of object: user, post, comment, token, file, etc." },
                            "object_id": { "type": "string", "description": "The ID or identifier of the created object." },
                            "description": { "type": "string" },
                            "rollback_method": { "type": "string", "description": "HTTP method for cleanup, e.g. DELETE." },
                            "rollback_url": { "type": "string", "description": "Full URL to call for cleanup." },
                            "rollback_body": { "type": "string", "description": "Optional request body for cleanup." }
                        },
                        "required": ["domain", "object_type", "object_id"]
                    }
                },
                {
                    "name": "rollback_test_object",
                    "description": "Execute cleanup for a previously claimed test object. Sends the rollback HTTP request and marks the object as rolled_back.",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "id": { "type": "integer", "description": "ID of the test object to rollback." }
                        },
                        "required": ["id"]
                    }
                },
                {
                    "name": "get_test_objects",
                    "description": "List all claimed test objects for a target. Filter by status to find objects that still need cleanup.",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "domain": { "type": "string" },
                            "status": { "type": "string", "enum": ["active", "rolled_back", "orphaned"] }
                        },
                        "required": ["domain"]
                    }
                },
                {
                    "name": "bulk_upsert_coverage",
                    "description": "Batch update coverage for multiple endpoint+vector pairs in a single call. Much faster than individual upsert_coverage calls.",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "items": {
                                "type": "array",
                                "items": {
                                    "type": "object",
                                    "properties": {
                                        "endpoint_id": { "type": "integer" },
                                        "vector": { "type": "string", "enum": ["sqli", "xss", "ssrf", "csrf", "idor", "bola", "rce", "lfi", "xxe", "ssti", "auth", "cors", "other"] },
                                        "status": { "type": "string", "enum": ["pending", "in_progress", "done", "skipped"] },
                                        "description": { "type": "string" },
                                        "notes": { "type": "string" }
                                    },
                                    "required": ["endpoint_id", "vector", "status"]
                                }
                            }
                        },
                        "required": ["items"]
                    }
                },
                {
                    "name": "bulk_save_requests",
                    "description": "Save multiple HTTP request/response pairs in a single call. Returns all created IDs.",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "items": {
                                "type": "array",
                                "items": {
                                    "type": "object",
                                    "properties": {
                                        "endpoint_id": { "type": "integer" },
                                        "raw_request": { "type": "string" },
                                        "raw_response": { "type": "string" },
                                        "status_code": { "type": "integer" },
                                        "response_time_ms": { "type": "integer" },
                                        "description": { "type": "string" },
                                        "notes": { "type": "string" }
                                    },
                                    "required": ["endpoint_id", "raw_request"]
                                }
                            }
                        },
                        "required": ["items"]
                    }
                },
                {
                    "name": "save_endpoint_example",
                    "description": "Save the minimal valid request/response example for an endpoint. Acts as functional API documentation. Only one example per endpoint (overwrites previous).",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "endpoint_id": { "type": "integer" },
                            "raw_request": { "type": "string" },
                            "raw_response": { "type": "string" },
                            "status_code": { "type": "integer" },
                            "description": { "type": "string" }
                        },
                        "required": ["endpoint_id", "raw_request"]
                    }
                },
                {
                    "name": "get_endpoint_example",
                    "description": "Retrieve the saved valid request/response example for an endpoint.",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "endpoint_id": { "type": "integer" }
                        },
                        "required": ["endpoint_id"]
                    }
                },
                {
                    "name": "parse_api_spec",
                    "description": "Download and parse an OpenAPI/Swagger specification (JSON) and automatically import all discovered routes into the database's endpoints table.",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "domain": { "type": "string", "description": "The target domain to link endpoints to." },
                            "url": { "type": "string", "description": "URL to the swagger.json file." },
                            "json": { "type": "string", "description": "Raw JSON string if URL is not available." }
                        },
                        "required": ["domain"]
                    }
                },
                {
                    "name": "parse_graphql_spec",
                    "description": "Download and parse a GraphQL Introspection JSON and automatically import all discovered queries and mutations into the database's endpoints table.",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "domain": { "type": "string", "description": "The target domain to link endpoints to." },
                            "url": { "type": "string", "description": "URL to the GraphQL introspection JSON file." },
                            "json": { "type": "string", "description": "Raw JSON string if URL is not available." },
                            "base_endpoint": { "type": "string", "description": "The base path for GraphQL, e.g. '/graphql'. Default is '/graphql'." }
                        },
                        "required": ["domain"]
                    }
                },
                {
                    "name": "get_phase_playbook",
                    "description": "Get current phase, checklist, transition options, and recalled lessons. Call at session start.",
                    "inputSchema": { "type": "object", "properties": { "domain": { "type": "string" } }, "required": ["domain"] }
                },
                {
                    "name": "transition_phase",
                    "description": "Explicitly move to a new methodology phase. Phases: setup, recon, enumeration, vuln_mapping, exploitation, post_exploitation, reporting.",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "domain": { "type": "string" },
                            "to_phase": { "type": "string", "enum": ["setup","recon","enumeration","vuln_mapping","exploitation","post_exploitation","reporting"] },
                            "reason": { "type": "string" }
                        },
                        "required": ["domain", "to_phase"]
                    }
                },
                {
                    "name": "save_hypothesis",
                    "description": "Record an attack hypothesis to track. Update its status as you test it.",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "domain": { "type": "string" },
                            "hypothesis": { "type": "string", "description": "The attack idea or question" },
                            "source": { "type": "string", "description": "What triggered this hypothesis" }
                        },
                        "required": ["domain", "hypothesis"]
                    }
                },
                {
                    "name": "get_hypotheses",
                    "description": "List tracked hypotheses for a target.",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "domain": { "type": "string" },
                            "status": { "type": "string", "enum": ["open","testing","confirmed","rejected"] }
                        },
                        "required": ["domain"]
                    }
                },
                {
                    "name": "update_hypothesis",
                    "description": "Update hypothesis status or add evidence.",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "id": { "type": "integer" },
                            "status": { "type": "string", "enum": ["open","testing","confirmed","rejected"] },
                            "evidence": { "type": "string" }
                        },
                        "required": ["id"]
                    }
                },
                {
                    "name": "save_dead_end",
                    "description": "Record a ruled-out technique to prevent re-exploration. Include assumption_tested and expected_if_vulnerable.",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "domain": { "type": "string" },
                            "technique": { "type": "string", "description": "What was tried" },
                            "target_info": { "type": "string", "description": "Against what (endpoint, service, etc.)" },
                            "assumption_tested": { "type": "string", "description": "What security assumption were you testing? e.g. 'the aud claim is validated by transfer-service'" },
                            "expected_if_vulnerable": { "type": "string", "description": "What response/behavior would have confirmed the assumption is wrong? e.g. '200 OK with transfer executed instead of 401'" },
                            "reason": { "type": "string", "description": "What actually happened and why this rules out the assumption" }
                        },
                        "required": ["domain", "technique", "reason"]
                    }
                },
                {
                    "name": "recall_engagement_state",
                    "description": "Filtered engagement snapshot. lens: progress|hosts|creds|open_hypotheses|dead_ends|attack_surface|technique_gaps.",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "domain": { "type": "string" },
                            "lens": { "type": "string", "enum": ["progress","hosts","creds","open_hypotheses","dead_ends","attack_surface","technique_gaps"] }
                        },
                        "required": ["domain", "lens"]
                    }
                },
                {
                    "name": "record_lesson",
                    "description": "Record a lesson into episodic memory for future recall. Include domain_type to avoid cross-domain noise.",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "domain": { "type": "string" },
                            "trigger_pattern": { "type": "string", "description": "What was observed" },
                            "hypothesis": { "type": "string", "description": "What was hypothesized" },
                            "action_taken": { "type": "string", "description": "What was done" },
                            "outcome": { "type": "string", "description": "What happened" },
                            "lesson": { "type": "string", "description": "Generalized takeaway" },
                            "tags": { "type": "array", "items": { "type": "string" } },
                            "domain_type": { "type": "string", "enum": ["web","binary","cloud","infra","mobile"], "description": "Type of engagement - used to filter lessons so cloud lessons don't surface in web searches and vice versa" }
                        },
                        "required": ["domain", "trigger_pattern", "lesson"]
                    }
                },
                {
                    "name": "recall_similar_situations",
                    "description": "Search episodic memory for lessons matching a situation. Pass domain_type to filter.",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "situation": { "type": "string", "description": "Describe what you're seeing now" },
                            "limit": { "type": "integer", "description": "Max results (default 5)" },
                            "domain_type": { "type": "string", "enum": ["web","binary","cloud","infra","mobile"], "description": "Filter to lessons from the same type of engagement" }
                        },
                        "required": ["situation"]
                    }
                },
                {
                    "name": "generate_report",
                    "description": "Generate a structured audit report: scope, findings by severity, coverage summary, appendix.",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "domain": { "type": "string", "description": "Target domain to generate report for" }
                        },
                        "required": ["domain"]
                    }
                },
                {
                    "name": "track_agent",
                    "description": "Register a sub-agent tracking record in DB (does NOT spawn a real process). Returns ID to pass to the spawned agent. Sweeps old finished records on call.",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "domain":    { "type": "string", "description": "Target domain this agent is working on" },
                            "role":      { "type": "string", "description": "Short label for what this agent does, e.g. 'auth-mapper', 'git-history', 'recon'" },
                            "objective": { "type": "string", "description": "One sentence: exactly what this agent should find or produce" }
                        },
                        "required": ["domain", "role", "objective"]
                    }
                },
                {
                    "name": "update_agent_status",
                    "description": "Called by sub-agent on finish/fail, or by orchestrator to cancel. Include artifact IDs created.",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "id":           { "type": "string", "description": "Agent ID from track_agent" },
                            "status":       { "type": "string", "enum": ["active", "done", "failed", "cancelled"] },
                            "summary":      { "type": "string", "description": "What the agent found or why it failed" },
                            "artifact_ids": { "type": "array", "items": { "type": "string" }, "description": "IDs of hypotheses/findings created during this run" }
                        },
                        "required": ["id", "status"]
                    }
                },
                {
                    "name": "list_agents",
                    "description": "List agents for a domain. Default: active only. all=true includes recently completed.",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "domain": { "type": "string" },
                            "all":    { "type": "boolean", "description": "Include completed agents (default: false, active only)" }
                        },
                        "required": ["domain"]
                    }
                }
            ]
        }
    })
}
