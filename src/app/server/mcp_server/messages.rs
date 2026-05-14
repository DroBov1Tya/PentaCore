use serde_json::{Value, json};

pub fn initialize_msg(id: &Value) -> Value {
    json!({
        "jsonrpc": "2.0",
        "id": id,
        "result": {
            "protocolVersion": "2024-11-05",
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
Persistent context store for pentest sessions. Saves tokens by giving structured, queryable memory across sessions.
**NOTE TO AI:** You can use this MCP server OR you can make standard HTTP REST requests to localhost:{} if you find it more convenient. Both methods work and modify the same database.

### Rules
- ALWAYS start session with `get_scope`
- A finding is confirmed only with a reproducible PoC — use status=potential until then
- Save raw request and response for every finding — this is your evidence base
- No findings means incomplete coverage, not a clean target
- Check coverage before closing a phase

### Networking & Sessions
- Use `set_session` to globally configure authentication tokens and cookies.
- Use `make_request` to execute HTTP calls automatically using the global session.
  - If you omit `user_agent`, it will be randomized on every request.
- Use `revoke_session` if you encounter 401/403 errors and need to clear stale context.
- Use `make_race_requests` to send parallel requests to test for Race Conditions.", server_path.split(':').last().unwrap_or("8082"))
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
                    "name": "get_summary",
                    "description": "Full target picture in one request. Restores session context.",
                    "inputSchema": { "type": "object", "properties": { "domain": { "type": "string" } }, "required": ["domain"] }
                },
                {
                    "name": "get_scope",
                    "description": "Get engagement rules and scope for a target.",
                    "inputSchema": { "type": "object", "properties": { "domain": { "type": "string" } }, "required": ["domain"] }
                },
                {
                    "name": "save_scope",
                    "description": "Save or update engagement rules.",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "domain": { "type": "string" }, "objective": { "type": "string" },
                            "in_scope": { "type": "string" }, "out_of_scope": { "type": "string" }, "rules": { "type": "string" }
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
                            "endpoint_id": { "type": "integer" }, "vector": { "type": "string" },
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
                    "description": "Make an HTTP request using global session context and automatically save it to the DB.",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "method": { "type": "string" },
                            "url": { "type": "string" },
                            "body": { "type": "string" },
                            "cookies": { "type": "array", "items": { "type": "string" } },
                            "proxy": { "type": "string" },
                            "user_agent": { "type": "string", "description": "Optional custom User-Agent. If omitted, a random browser UA is used." },
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
                                        "vector": { "type": "string" },
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
                }
            ]
        }
    })
}
