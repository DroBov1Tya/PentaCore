# HackStorage

**Persistent context storage for AI-powered penetration testing agents.**

HackStorage is a lightweight Rust server that gives pentest AI agents structured, queryable memory across sessions. Instead of losing state on every restart or wasting tokens re-reading files, agents can store and retrieve exactly what they need with a single HTTP request.

## Why

LLM-based pentest agents have a fundamental problem: **limited context windows and no persistence**. Every new session starts from zero. HackStorage solves this by acting as a shared database that agents read from and write to throughout an engagement.

One request restores full context. One request saves a finding. No files, no prompt engineering — just data.

## What It Stores

| Entity | Purpose |
|--------|---------|
| **Targets** | Domains under test |
| **Scope** | Engagement rules — what's allowed, what's forbidden |
| **Endpoints** | Discovered API routes and pages |
| **Requests** | Raw HTTP request/response pairs (evidence) |
| **Findings** | Vulnerabilities with severity, status, payload, and proof |
| **Credentials** | Discovered auth tokens, passwords, API keys |
| **Coverage** | Per-endpoint vector testing progress (SQLi, XSS, SSRF, ...) |
| **Attack Chains** | Multi-step kill chains linking findings into attack sequences |
| **Target Relations** | Domain relationships — subdomains, shared infra, pivot points |

## Quick Start

### Prerequisites

- Rust 1.85+ (edition 2024)
- SQLite

### Setup

```bash
git clone https://github.com/DroBoV1tya/hackstorage_mcp.git
cd hackstorage_mcp

# Configure environment
cp bin/.env.example bin/.env
# Edit bin/.env — set DB_LOCATION and SERVER_PATH

# Build and run
cargo build --release
./target/release/HackStorage
```

### Environment Variables

| Variable | Default | Description |
|----------|---------|-------------|
| `DB_LOCATION` | `sqlite:./db/mcp.db` | SQLite database path |
| `SERVER_PATH` | `localhost:8082` | Server bind address (TCP or Unix socket) |

## API Reference

### Session Start (agent workflow)

```
1. GET  /targets/{domain}/scope     — read engagement rules before anything
2. GET  /targets/{domain}/summary   — restore full context in one request
3. GET  /targets/{domain}/relations — check domain graph, plan pivots
4. GET  /endpoints/{id}/coverage?status=pending — find untested vectors
```

### During Work

```
POST /targets/{domain}/endpoints         — save discovered endpoint
POST /endpoints/{id}/requests            — save raw HTTP request/response
POST /endpoints/{id}/coverage            — update vector test progress
POST /targets/{domain}/findings          — save a vulnerability
POST /targets/{domain}/chains            — create attack chain
POST /chains/{id}/steps                  — link findings into chain
POST /targets/{domain}/credentials       — save discovered credential
```

### Self-Documentation

```
GET /   — returns full API documentation with schemas, examples, and rules
```

### Example: Save a Finding

```bash
curl -X POST http://localhost:8082/targets/example.com/findings \
  -H "Content-Type: application/json" \
  -d '{
    "type": "sqli",
    "severity": "high",
    "status": "confirmed",
    "endpoint_id": 42,
    "payload": "1'\'' OR 1=1--",
    "evidence": "HTTP 200 — full user table returned",
    "description": "Blind SQLi in search parameter, union-based extraction possible"
  }'
```

### Example: Get Target Summary

```bash
curl http://localhost:8082/targets/example.com/summary
```

```json
{
  "domain": "example.com",
  "endpoints_total": 147,
  "endpoints_200": 89,
  "findings_total": 12,
  "critical": 1,
  "high": 3,
  "medium": 5,
  "low": 3,
  "confirmed": 4,
  "potential": 8
}
```

## Architecture

```
src/
├── main.rs              # Entry point
├── app.rs               # Application lifecycle and graceful shutdown
├── config.rs            # Environment-based configuration
├── constants.rs         # Version info
├── api/
│   ├── server.rs        # Axum server with DualListener (TCP + Unix socket)
│   ├── routes.rs        # Route definitions
│   └── handlers/        # One handler per entity
│       ├── init.rs      # MCP handshake + self-documenting instructions
│       ├── scope.rs
│       ├── summary.rs
│       ├── endpoints.rs
│       ├── findings.rs
│       ├── credentials.rs
│       ├── requests.rs
│       ├── coverage.rs
│       ├── attack_chains.rs
│       └── target_relations.rs
└── app/
    └── database/
        ├── pool.rs      # SQLite pool with WAL + foreign keys
        ├── migrations/  # SQL schema
        └── queries/     # One query module per entity
```

## Roadmap

- [ ] **Auto-Report Generation** — `GET /targets/{domain}/report` producing structured pentest reports from stored data
- [ ] **Telegram Notifications** — real-time alerts via Telegram bot when critical findings are confirmed
- [ ] **Multi-agent tracking** — `agent_id` on findings/requests to attribute discoveries
- [ ] **Finding deduplication** — detect when multiple agents report the same vulnerability
- [ ] **Export formats** — Nuclei templates, Burp XML, SARIF

## License

MIT — see [LICENSE](LICENSE) for details.
