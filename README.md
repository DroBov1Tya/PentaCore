<div align="center">
  <img src="https://raw.githubusercontent.com/tandpfun/skill-icons/main/icons/Rust.svg" width="60" />
  <h1>PentaCore MCP</h1>
  <p><strong>The Autonomous Persistent Memory & Offensive Tooling Server for Pentest AI Agents</strong></p>
  
  [![Rust](https://img.shields.io/badge/Rust-1.85+-orange.svg)](https://www.rust-lang.org)
  [![MCP Compatible](https://img.shields.io/badge/MCP-Ready-blue.svg)](https://modelcontextprotocol.io/)
  [![License](https://img.shields.io/badge/License-MIT-green.svg)](LICENSE)
</div>

---

**PentaCore** is a dual-transport (Stdio & REST) server written in Rust. It equips Large Language Model (LLM) agents with **persistent memory** and **offensive reconnaissance capabilities**. 

Instead of losing state on every restart or wasting context window tokens re-reading files, agents can store exact findings and retrieve scoped context with a single query. Furthermore, PentaCore reduces token consumption by offloading repetitive recon tasks (like DNS brute-forcing and passive secret extraction) directly into the Rust server.

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
| **Dual Transport** | Speaks standard JSON-RPC via `stdio` (MCP spec) and HTTP REST. Negotiates protocols automatically (`2024-11-05` to `2025-11-25`). |
| **Active Recon** | Integrated `enumerate_subdomains` (Tokio/Hickory-DNS) and `resolve_dns` (A, AAAA, MX, TXT, NS). |
| **Passive Intelligence** | Extracts API routes and secrets from HTTP responses automatically. |
| **Memory & RAG** | Uses LanceDB and FastEmbed for high-speed semantic search over past findings and architectural notes. |
| **Data Models** | Structured tables for Targets, Endpoints, Findings, Credentials, Coverage, and Attack Chains. |
| **Vulnerability Triage** | Updates findings, enforcing strict Quality Gates (e.g., `confirmed` status requires `evidence`). |
| **Replay & Diff Engine** | `replay_as` automates IDOR/Auth testing by stripping/injecting tokens. `diff_requests` natively compares responses to highlight security discrepancies. |
| **Safe Parsing** | 500KB response truncation protects the agent's context window from blowing up on large binaries/dumps. |

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

When connected as an MCP server, PentaCore exposes numerous tools directly to the AI, including:

- **`make_request`**: Perform HTTP requests (GET/POST/PUT/DELETE) with custom headers, proxy routing, and automatic passive analysis.
- **`replay_as`**: Automatically replay a saved request with new cookies/auth tokens (ideal for IDORs).
- **`diff_requests`**: Compare two HTTP responses to find subtle body/status/timing differences.
- **`memorize_concept` / `search_knowledge`**: RAG integration to store and semantically retrieve domain knowledge.
- **`resolve_dns` / `enumerate_subdomains`**: Built-in reconnaissance.
- **`save_finding` / `update_finding`**: Persist a vulnerability into SQLite. Features strict Quality Gates.
- **`bulk_upsert_coverage` / `bulk_save_requests`**: Fast, batch database operations for scaling recon imports.

---

## 🗺️ Roadmap: The Path to Autonomous Pentesting

These upcoming features are designed to extend PentaCore's autonomous pentesting capabilities.

### 1. Advanced Fuzzing & Wordlists (`smart_fuzz`)
- **Concept:** Provide a built-in dictionary of the 50-100 most critical endpoints (e.g., `/.git/`, `/.env`, `/api/v1/users`, `/swagger-ui.html`) and subdomains.
- **Feature:** A `fuzz_endpoint` tool that takes a base URL and blasts it concurrently with mutations, returning only successful (200/403) or anomalous responses.

### 2. GraphQL & REST API Analyzer
- **Concept:** Stop wasting AI context tokens on 50KB GraphQL Introspection schemas or OpenAPI specs.
- **Feature:** Implement an `analyze_api` tool. Rust fetches the schema, stores the raw JSON in the database, and returns a highly condensed summary to the AI.

### 3. DOM XSS Headless Sniper
- **Concept:** Verify XSS automatically.
- **Feature:** Spin up a headless browser (Puppeteer/Playwright wrapper), inject the payload, and capture a screenshot of the `alert()` execution as PoC.

### 4. Automated Workflow Orchestration
- **`get_next_targets`:** Smart routing to retrieve endpoints with missing or low test coverage.
- **`safe_rate_limiter`:** Built-in rate limiting for `make_request` and `make_race_requests`.

### 5. OAST & Reporting
- **`oast_session`:** Out-of-Band Application Security Testing support via generated markers.
- **`report_export`:** One-click generation of comprehensive Markdown/JSON pentest reports.

*(Note: Network port scanning is deferred, as dedicated tools like RustScan/Nmap are better suited for network-level operations.)*

---

## 🛡 License
MIT — see [LICENSE](LICENSE) for details.
