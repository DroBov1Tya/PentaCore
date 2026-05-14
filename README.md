<div align="center">
  <img src="https://raw.githubusercontent.com/tandpfun/skill-icons/main/icons/Rust.svg" width="60" />
  <h1>PentaCore MCP</h1>
  <p><strong>The Autonomous Persistent Memory & Offensive Tooling Server for Pentest AI Agents</strong></p>
  
  [![Rust](https://img.shields.io/badge/Rust-1.85+-orange.svg)](https://www.rust-lang.org)
  [![MCP Compatible](https://img.shields.io/badge/MCP-Ready-blue.svg)](https://modelcontextprotocol.io/)
  [![License](https://img.shields.io/badge/License-MIT-green.svg)](LICENSE)
</div>

---

**PentaCore** is a high-performance, dual-transport (Stdio & REST) server written in Rust. It equips Large Language Model (LLM) agents with **persistent memory** and **autonomous offensive reconnaissance capabilities**. 

Instead of losing state on every restart or wasting context window tokens re-reading files, agents can store exact findings and retrieve scoped context with a single query. Furthermore, PentaCore drastically reduces token consumption by offloading repetitive recon tasks (like DNS brute-forcing and passive secret extraction) directly into the Rust server.

## ⚡ Why PentaCore?

LLM-based pentest agents suffer from limited context windows and lack of persistence. Every new session starts from zero. 

PentaCore solves this by acting as a shared brain and autonomous assistant:
1. **Context Persistence:** One request restores full context. One request saves a finding. No files, no prompt engineering — just data.
2. **Token Efficiency:** Stop wasting tokens writing one-off Python scripts for DNS enumeration or schema parsing. PentaCore runs it locally, at native speeds, and returns highly-condensed summaries to the AI.
3. **Passive Intelligence:** PentaCore transparently inspects outgoing HTTP responses for leaked secrets (JWTs, API keys) and alerts the AI automatically.

---

## 🛠 Features

| Feature | Purpose |
|---------|---------|
| **Dual Transport** | Speaks standard JSON-RPC via `stdio` (MCP spec) and HTTP REST. |
| **Active Recon** | Integrated `enumerate_subdomains` (Tokio/Hickory-DNS) and `resolve_dns` (A, AAAA, MX, TXT, NS). |
| **Passive Recon** | Extracts API routes and secrets from HTTP responses automatically. |
| **Data Models** | Structured tables for Targets, Endpoints, Findings, Credentials, Coverage, and Attack Chains. |
| **Session Control** | AI can manage its own global `cookies` and `auth_token` for authenticated testing. |

---

## 🚀 Quick Start

### Prerequisites
- Rust 1.85+
- SQLite

### Setup
```bash
git clone https://github.com/DroBoV1tya/PentaCore_mcp.git
cd PentaCore_mcp

# Build the release binary
cargo build --release

# Run as a standalone REST server...
./target/release/PentaCore

# ...or add it to your Claude Desktop config (claude_desktop_config.json)
{
  "mcpServers": {
    "PentaCore": {
      "command": "/path/to/PentaCore_mcp/target/release/PentaCore",
      "args": [],
      "env": {
        "DB_LOCATION": "sqlite:/path/to/PentaCore_mcp/db/mcp.db"
      }
    }
  }
}
```

---

## 📡 Core Tools (MCP Interface)

When connected as an MCP server, PentaCore exposes the following tools directly to the AI:

- **`make_request`**: Perform HTTP requests (GET/POST/PUT/DELETE) with custom headers, proxy routing, and automatic passive analysis.
- **`resolve_dns`**: Perform standard public DNS resolution (like `dig` or `nslookup`).
- **`enumerate_subdomains`**: Brute-force discovery of hidden infrastructure using a built-in top 50 list.
- **`set_session` / `revoke_session`**: Automatically apply cookies and authorization headers globally to all outgoing requests.
- **`save_finding`**: Persist a confirmed vulnerability directly into the SQLite database.

---

## 🗺️ Roadmap: The Path to Autonomous Pentesting

These upcoming features are designed to transform PentaCore into a deadly, fully autonomous pentest agent, drastically reducing manual AI scripting.

### 1. Advanced Fuzzing & Wordlists (`smart_fuzz`)
- **Concept:** Provide a built-in dictionary of the 50-100 most critical endpoints (e.g., `/.git/`, `/.env`, `/api/v1/users`, `/swagger-ui.html`) and subdomains.
- **Feature:** A `fuzz_endpoint` tool that takes a base URL and blasts it concurrently with mutations, returning only successful (200/403) or anomalous responses.
- **Customization:** Support reading larger wordlists from local files.

### 2. GraphQL & REST API Analyzer
- **Concept:** Stop wasting AI context tokens on 50KB GraphQL Introspection schemas or OpenAPI specs.
- **Feature:** Implement an `analyze_api` tool. Rust fetches the schema, stores the raw JSON in the database, and returns a highly condensed summary to the AI (e.g., *"Found 40 queries, 12 mutations. Focus on: `adminUpdateUser`"*).

### 3. Smart Secret Extractor (Passive)
- **Concept:** AI should never miss a leaked token.
- **Feature:** Add regex-based passive scanning to `make_request`. Every HTTP response is scanned for JWTs, AWS Keys, and Private Keys. If found, automatically save to the `credentials` table and alert the AI.

### 4. DOM XSS Headless Sniper
- **Concept:** Verify XSS automatically.
- **Feature:** Spin up a headless browser (Puppeteer/Playwright wrapper), inject the payload, and capture a screenshot of the `alert()` execution as PoC.

### 5. Automated Workflow Orchestration
- **`get_next_targets`:** Smart routing to retrieve endpoints with missing or low test coverage, filterable by `state_changing`, `auth_required`, `service`, and `risk`.
- **`bulk_upsert_coverage` & `bulk_save_requests`:** Batch processing tools to eliminate the latency of logging findings one by one.
- **`safe_rate_limiter`:** Built-in rate limiting for `make_request` and `make_race_requests` to ensure safe operation without relying on AI prompt discipline.

### 6. Vulnerability & Object Management
- **`diff_requests`:** Native comparison of two HTTP responses (status, size, headers, JSON structure) to automatically flag IDORs or Blind injections.
- **`claim_test_object`:** A registry for artifacts created during testing (e.g., test users, mock posts) linked with a `rollback_command` for machine-verifiable cleanup.
- **`redact_secret_fields`:** Automatic redaction of auth tokens, cookies, and passwords from saved HTTP evidence to ensure report cleanliness.

### 7. OAST & Reporting
- **`oast_session`:** Out-of-Band Application Security Testing support via generated markers, polling mechanisms, and callback binding to specific `request_id`s (for Blind SSRF/XSS).
- **`report_export`:** One-click generation of comprehensive Markdown/JSON pentest reports synthesizing findings, requests, coverage, and attack chains.
- **`endpoint_examples`:** Automatic retention of the smallest valid request/response pair for each endpoint to serve as functional documentation.

*(Note: Network port scanning is deferred, as dedicated tools like RustScan/Nmap are better suited for network-level operations.)*

---

## 🛡 License
MIT — see [LICENSE](LICENSE) for details.
