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
            }
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
                "text": format!("## PentaCore MCP
Persistent context store for pentest sessions with methodology-driven workflow.
**NOTE TO AI:** You can use this MCP server OR you can make standard HTTP REST requests to localhost:{} if you find it more convenient. Both methods work and modify the same database.

### Operational Cycle (OODA)
Every action follows this loop:
1. Observe - call recall_engagement_state to see current state
2. Orient - for the component or surface you are analyzing, do this BEFORE calling any tools:
   - State out loud: 'This component assumes X about its input / caller / environment'
   - For each assumption ask: is this ENFORCED in code, or just EXPECTED to be true?
   - The gap between EXPECTED and ENFORCED is where bugs live
   - Then call get_phase_playbook and search_knowledge to pull techniques relevant to those assumptions
3. Decide - pick the assumption most likely to be wrong; check recall_similar_situations for prior cases where this assumption failed
4. Act - test THAT specific assumption; know in advance what a vulnerable response looks like vs a safe one
5. Reflect - call record_lesson with structured outcome; use save_dead_end if it failed

Variant propagation rule: when you confirm a finding, immediately ask 'where else does the same root cause exist?' - the same developer wrote multiple components, the same assumption is likely wrong in more than one place. Save follow-up hypotheses before moving on.

### Knowledge Base (RAG) - USE THIS PROACTIVELY
The knowledge base contains pre-loaded security research techniques, mental models, and methodologies. **You MUST query it before starting any non-trivial task.**

**Mandatory triggers - call 'search_knowledge' when you:**
- Start analyzing an unknown service, protocol, or codebase
- Are about to write a PoC or exploit
- See an interesting pattern (e.g. length field, auth check, state transition, crypto usage)
- Don't know where to start on a target
- Need a checklist for a vulnerability class
- Want to understand how to approach binary/protocol research vs web testing

**Pre-loaded categories:**
- 'mindset' - Mental models: attack surface decomposition, state machine confusion, differential analysis (patch diffing), taint analysis, STRIDE threat modeling, hypothesis-driven research, the 'what if attacker controls X' framework
- 'technique' - Specific attack techniques: auth logic bugs, JWT, OAuth 2.0/OIDC, SAML/XSW, SSRF, request smuggling, cache poisoning, GraphQL, async pipeline races; API security (mass assignment, BOLA/BFLA, excessive data exposure, rate-limit bypass); business logic (workflow bypass, value manipulation, race-on-logic, coupon/quota abuse); mobile (Android/iOS, Frida, SSL pinning); cloud-native (Kubernetes RBAC, IAM privesc, container escape); binary exploitation (heap, ROP, format string)
- 'methodology' - Process guides: web whitebox/blackbox checklists, binary analysis, docker image auditing, infra/network pentesting, stack fingerprinting -> CVE -> exploit, patch diffing, code review for zero-days
- 'tools' - Tool reference and usage: recon tooling, web fuzzing, vulnerability scanning, network and AD tools, pivoting and C2, exposed services detection

domain parameter:
- To search the global knowledge base: omit domain or pass domain: global
- To search engagement-specific memories: pass the target domain
- Mixing these loses global techniques - always query global KB separately first

Example queries that work well:
- search_knowledge(query: how to find bugs in protocol length fields, domain: global)
- search_knowledge(query: authentication bypass logic bugs, domain: global)
- search_knowledge(query: first steps when analyzing unknown codebase, domain: global)
- search_knowledge(query: integer overflow malloc, domain: global)
- search_knowledge(query: race condition TOCTOU, domain: global)
- search_knowledge(query: oauth jwt attack, domain: global)
- search_knowledge(query: RBAC privilege escalation authorization bypass, domain: global)

To add your own knowledge: use memorize_concept(domain: global, category: technique or mindset or methodology) - it will be available to all future sessions.

### Agent Orchestration
When a task can be parallelized, spawn sub-agents instead of doing everything sequentially yourself.

Orchestrator role: decompose -> assign -> evaluate results -> synthesize. Do NOT execute tasks yourself when an agent can do it.

Pattern:
1. spawn_agent(domain, role, objective) -> get agent ID
2. Launch the sub-agent with a narrow prompt that includes the ID
3. Sub-agent: reads recall_engagement_state(), works, calls update_agent_status(id, done, summary, artifact_ids)
4. Orchestrator: list_agents(domain) to check completion, then recall_engagement_state() to read what they found

Sub-agent prompt must include:
- The agent ID and instruction to call update_agent_status when done
- Explicit scope + explicit DO NOT boundary
- recall_engagement_state() at start, save_hypothesis() and save_dead_end() during work

Finished agents older than 1 hour are deleted on the next spawn_agent call. No cleanup needed.

### Session Start Sequence (mandatory, in this order)
Every new engagement begins with three calls - no exceptions:
1. get_scope(domain) - load rules and objective for this target
2. get_phase_playbook(domain) - understand current methodology phase and what to do next
3. search_knowledge with the query from the routing table below - pull the step-by-step checklist for your scenario

Routing table - pick the query that matches what you have:

| Scenario | Query to pass to search_knowledge |
|----------|------------------------------------|
| Web app, no source code | blackbox web no source crawl surface map IDOR role matrix business logic |
| Web app + source code | git history auth flow grep sinks taint analysis source code review |
| Source code only, no live system | static analysis grep secrets sinks code audit no live app |
| Binary / executable | checksec heap overflow rop format string memory corruption pwntools |
| Mobile app (APK / IPA) | mobile android ios apk frida ssl pinning decompile exported component deeplink |
| Kubernetes / cloud / containers | kubernetes service account iam container escape imds privilege escalation cloud |
| Docker image / container | trivy container image layers secrets escape entrypoint |
| Corporate network / infra | active directory lateral movement nmap smb kerberos spray |

Run a second search after the checklist if your target spans areas - e.g. a mobile app also needs the API tested (use the web query too).

Almost every web/mobile/API target ALSO needs these two - search them after the scenario checklist:
- API testing: search 'API mass assignment BOLA BFLA excessive data exposure rate limit bypass versioning'
- Business logic (highest-value, scanner-blind): search 'business logic workflow bypass value manipulation race coupon quota replay'

Do not search for 'where to start' or 'checklist' - those queries find the routing table itself, not the checklists.

### Rules
- ALWAYS execute the Session Start Sequence above before any other action
- If get_scope returns \"Scope not found\": call save_scope(domain, objective, in_scope) immediately, then continue the sequence
- ALWAYS call 'search_knowledge' at session start AND whenever you encounter a new attack surface or technique boundary
- Use 'save_hypothesis' to track attack ideas - update status as you test them
- A finding is confirmed only with a reproducible PoC - use status=potential until then
- Save raw request and response for every finding - this is your evidence base
- Use 'save_dead_end' when a technique fails - prevents re-exploration loops
- Use 'transition_phase' to move between methodology phases explicitly
- No findings means incomplete coverage, not a clean target

### Networking & Sessions
- Use 'set_session' to globally configure authentication tokens and cookies.
- Use 'make_request' to execute HTTP calls automatically using the global session.
  - If you omit 'user_agent', it will be randomized on every request.
- Use 'revoke_session' if you encounter 401/403 errors and need to clear stale context.
- Use 'make_race_requests' to send parallel requests to test for Race Conditions.", server_path.split(':').last().unwrap_or("8082"))
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
                    "description": "Save or update engagement rules. Always include domain_type so recalled lessons from other engagement types don't pollute your playbook.",
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
                    "description": "Store a concept, technique, observation, or finding into the RAG knowledge base for semantic retrieval. Use category='technique' for attack techniques, 'mindset' for mental models, 'methodology' for process guides, or any engagement-specific category. Use domain='global' for knowledge that should be available across all engagements.",
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
                    "description": "Search the knowledge base using semantic similarity. Contains pre-loaded security techniques (mindset, technique, methodology categories) plus engagement-specific memories. Call this BEFORE starting any analysis, writing PoC code, or approaching an unfamiliar attack surface - it surfaces relevant techniques and mental models automatically.",
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
                    "description": "Get current methodology phase, checklist, transition options, and auto-recalled lessons from past engagements. This is the 'brain' - call it to understand where you are and what to do next.",
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
                    "description": "Record a technique that was tried and ruled out. Prevents re-exploration loops. IMPORTANT: include assumption_tested and expected_if_vulnerable so future agents know the test was done correctly, not just abandoned.",
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
                    "description": "Get a filtered view of the engagement. Lens options: progress (stats + phase), hosts (subdomains + infra), creds (credentials), open_hypotheses, dead_ends, attack_surface (untested endpoints + pending vectors), technique_gaps (which vulnerability categories have not been tested at all).",
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
                    "description": "Record a structured lesson from this engagement into episodic memory. Will be auto-recalled in future similar situations. Include domain_type so lessons only surface when context matches.",
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
                    "description": "Search episodic memory for lessons from past engagements matching a situation description. Pass domain_type to avoid cross-domain noise.",
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
                    "description": "Generate a structured security audit report from all stored data: scope, findings (sorted by severity), coverage summary, and appendix with subdomains/credentials/cleanup status. Use this for final bug bounty submission.",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "domain": { "type": "string", "description": "Target domain to generate report for" }
                        },
                        "required": ["domain"]
                    }
                },
                {
                    "name": "spawn_agent",
                    "description": "Register a sub-agent and get its ID. Use this before launching a sub-agent so you can track its work. Returns an ID and a ready-to-paste block to include in the sub-agent's prompt. Old finished agents are swept automatically - no manual cleanup needed.",
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
                    "description": "Called by a sub-agent when it finishes (or fails). Also callable by the orchestrator to cancel a running agent. Include IDs of any hypotheses or findings the agent created.",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "id":           { "type": "string", "description": "Agent ID from spawn_agent" },
                            "status":       { "type": "string", "enum": ["active", "done", "failed", "cancelled"] },
                            "summary":      { "type": "string", "description": "What the agent found or why it failed" },
                            "artifact_ids": { "type": "array", "items": { "type": "string" }, "description": "IDs of hypotheses/findings created during this run" }
                        },
                        "required": ["id", "status"]
                    }
                },
                {
                    "name": "list_agents",
                    "description": "Show agents for a domain. By default shows only active agents. Use all=true to see recently completed ones. Use this to check whether sub-agents are done before reading their results.",
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
