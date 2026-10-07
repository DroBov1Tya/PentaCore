# pentacore

Persistent working memory for AI agents, served over the [Model Context Protocol](https://modelcontextprotocol.io).

An agent without memory goes in circles: it retries what already failed, loses its plan when the context is compacted, and scatters notes across scratch files nobody reads again. pentacore gives it one place to keep all of that, and to find it again by word, by field or by meaning.

It is a tool, not a manager. It stores what it is given and returns what is asked for. It never plans, ranks work or tells the agent what to do next.

- One local binary, no services to run. Data lives in a single directory.
- Built on the official Rust MCP SDK ([`rmcp`](https://github.com/modelcontextprotocol/rust-sdk)).
- Shared by any number of agents and projects at once.

## What it stores

| Store | Holds | Found by | Backed by |
|---|---|---|---|
| **Notes** | Narrative specifics: goals, steps, attempts and their outcomes, facts, decisions, open questions | Exact words (`find`) | SQLite + FTS5 |
| **Entities** | Operational state: the things being worked on, as structured records with a status, a confidence, attributes and named checks | Field filters (`query_entities`) | SQLite + JSON1 |
| **Concepts** | Generalised, reusable knowledge | Meaning (`search_concepts`) | LanceDB + local embeddings |

Notes form a tree through `parent` and a graph through typed links, which is how a goal is broken down and tracked. Entities form their own tree and graph. A note can point at the entity it is about, so the narrative stays attached to the state.

The split matters. "Show every endpoint that is `discovered`, has confidence above 0.7 and has not been through the `review` check" is a query over fields. It should not depend on how somebody phrased a note.

## Install

Requires Rust 1.88 or newer.

```sh
git clone <this repository>
cd pentacore
cargo build --release
```

The binary is `target/release/pentacore`. Copy it wherever you keep tools.

The first use of a concept tool downloads the embedding model (all-MiniLM-L6-v2, about 90 MB) into the data directory. Notes and entities work without it.

## Connect an agent

pentacore speaks MCP over stdio; the client launches it.

**Claude Code**

```sh
claude mcp add pentacore -- /absolute/path/to/pentacore
```

**Any MCP client** (the usual `mcpServers` block)

```json
{
  "mcpServers": {
    "pentacore": {
      "command": "/absolute/path/to/pentacore",
      "env": {
        "PENTACORE_PROJECT": "my-project"
      }
    }
  }
}
```

Then give the agent the usage guide in [`skills/pentacore-memory/SKILL.md`](skills/pentacore-memory/SKILL.md). For Claude Code, copy that directory into `~/.claude/skills/` or the project's `.claude/skills/`. For other agents, include its text in the system prompt.

## Configuration

Every variable is optional.

| Variable | Default | Meaning |
|---|---|---|
| `PENTACORE_HOME` | `~/.pentacore` | Data directory. Created with mode `0700`. |
| `PENTACORE_PROJECT` | name of the working directory | Project used when a call names none. |
| `PENTACORE_HTTP_ADDR` | unset (HTTP off) | Loopback address and port for the HTTP API. |
| `PENTACORE_HTTP_TOKEN` | unset | Bearer token for the HTTP API, at least 32 characters. Required when the address is set. |
| `RUST_LOG` | `warn,pentacore=info` | Log filter. Logs go to stderr. |

Variables can also come from a `.env` file **beside the executable**; see [`bin/.env.example`](bin/.env.example). A `.env` in the working directory is ignored on purpose: the working directory may be a checkout you do not trust.

## Tools

**Notes**

| Tool | Does |
|---|---|
| `note` | Record a goal, step, attempt, fact, decision or question. Reports similar existing notes. |
| `update_note` | Change fields, move under another parent, or `append` to the body. |
| `get_note` | One note in full, with children and links. |
| `find` | Full-text search. Falls back from exact words to word prefixes to any word. |
| `link` / `unlink` | Typed links: `depends_on`, `supports`, `contradicts`, `answers`, `supersedes`, `relates_to`. |
| `goal_graph` | The tree beneath a note, with every link touching it. |
| `forget_note` | Delete a note, or a subtree with `recursive`. |

**Entities**

| Tool | Does |
|---|---|
| `upsert_entity` | Create or update by `(project, type, key)`. Merges attributes; an identical write changes nothing. |
| `query_entities` | Filter, sort, count and group by column, `attrs.<name>` or `check.<name>`. |
| `mark_check` | Record the result of a named check. |
| `claim_entity` / `release_entity` | Reserve an entity for one agent, with a timeout. |
| `link_entities` | Named links between entities. |
| `entity_graph` | The tree beneath an entity, with its links. |
| `get_entity` | One entity with checks, children, links, notes about it and recent history. |
| `forget_entity` | Delete an entity, or a subtree with `recursive`. |

**Concepts**

| Tool | Does |
|---|---|
| `memorize_concept` | Store a concept. Reports existing ones that say nearly the same. |
| `search_concepts` | Semantic search. |
| `update_concept` / `forget_concept` | Maintain concepts. |

**Across stores**

| Tool | Does |
|---|---|
| `recall` | Search notes and concepts in one call. |
| `checkpoint` | Save a summary of where the work stands. |
| `resume` | Current state of a project: open goals, active steps, open questions, recent notes, entity counts, last checkpoint. |

Each tool carries a JSON Schema and MCP annotations (`readOnlyHint`, `destructiveHint`), so a client can show what a call will do before making it.

## Example

Break a goal down and record what happened:

```json
{"name": "note", "arguments": {"kind": "goal", "title": "Move session storage to Redis"}}
{"name": "note", "arguments": {"kind": "step", "title": "Replace the in-memory store", "parent": 1, "status": "active"}}
{"name": "note", "arguments": {"kind": "attempt", "title": "Swap the store behind the existing trait", "parent": 2,
  "status": "failed", "body": "Trait requires Sync; redis::Connection is not. Error E0277 in src/session/store.rs:41"}}
```

Track what is being worked through, and ask what is left:

```json
{"name": "upsert_entity", "arguments": {"type": "endpoint", "key": "POST /login", "status": "discovered",
  "confidence": 0.9, "attrs": {"auth": false, "handler": "auth::login"}}}
{"name": "mark_check", "arguments": {"id": 1, "name": "review", "result": "pass"}}
{"name": "query_entities", "arguments": {"type": "endpoint", "where": [
  {"field": "status", "op": "eq", "value": "discovered"},
  {"field": "confidence", "op": "gt", "value": 0.7},
  {"field": "check.review", "op": "is_null"}]}}
```

Pick up again in a new session:

```json
{"name": "resume"}
{"name": "find", "arguments": {"query": "E0277 session store"}}
```

## Projects and several agents

Everything belongs to a project. Reads default to the current project, and `"project": "*"` reads across all of them, so a lesson learned in one place can be found from another.

Several agents can write to the same project at the same time. Two mechanisms keep them out of each other's way:

- **Claims.** `claim_entity` returns a secret `claim_id`. Until the claim expires or is released, only calls that present it can change the entity. Of two simultaneous claims exactly one wins.
- **Authors.** Writing tools take an optional `author` label, recorded with the note, entity, check or event. It is informational and grants nothing.

A project is a namespace, not a security boundary: any agent connected to the same data directory can read and change any project. Use separate `PENTACORE_HOME` directories for memories that must not mix.

## HTTP API

Off by default. Set both `PENTACORE_HTTP_ADDR` and `PENTACORE_HTTP_TOKEN` to turn it on. It exposes the same tools as MCP:

```sh
curl -s http://127.0.0.1:8082/v1/tools/find \
  -H "Authorization: Bearer $PENTACORE_HTTP_TOKEN" \
  -H "Content-Type: application/json" \
  -d '{"query": "session store"}'
```

| Route | Does |
|---|---|
| `GET /health` | Liveness and version. |
| `GET /v1/tools` | Tool names, descriptions and schemas. |
| `POST /v1/tools/{name}` | Call a tool; the body is its arguments. |

Errors are [RFC 9457](https://www.rfc-editor.org/rfc/rfc9457) problem documents. Connect to the literal address: `localhost` is refused.

## Security

What is written to the memory is later read by an agent as context. An unauthorised write is therefore a prompt-injection vector, and an unauthorised read leaks whatever the agent has seen. The design follows from that.

- **stdio is the only transport by default.** The parent process is the only caller.
- **HTTP cannot be exposed by accident.** It binds loopback only, refuses to start without a token of at least 32 characters, requires the exact `Host` header (which stops DNS rebinding), rejects any request carrying an `Origin` header (it is not a browser API), and compares the token in constant time.
- **Input is validated at the boundary.** Every tool argument is parsed into a type that enforces its length and character set; unknown fields are rejected.
- **No caller text becomes SQL.** Values are bound parameters. Full-text queries are quoted, so FTS5 operators are plain text. Entity queries are assembled from an allowlist of fields and operators.
- **Bounded everywhere.** Message size, body size, result counts, tree depth and attribute counts all have limits.
- **Errors do not leak internals.** Callers see a short message; causes go to the log.
- **Data at rest** sits in a `0700` directory. Deleted rows are overwritten (`secure_delete`).

Known limits:

- Stored text is not sanitised. An agent that saves untrusted text will read it back later; the usage guide tells agents to treat recalled memory as data, not instructions.
- The HTTP API has no rate limiting and no header-read timeout.
- LanceDB keeps old table versions, so a forgotten concept remains recoverable from the data directory.
- Claims prevent accidental collisions between cooperating agents. They do not stop a hostile process that can open the database file.

## Development

```sh
cargo test                       # unit and integration tests
cargo clippy --all-targets -- -D warnings
cargo fmt --check
cargo audit
```

One test needs the embedding model and is skipped by default:

```sh
cargo test -- --include-ignored
```

Layout:

```
src/
  main.rs, config.rs        entry point and environment
  app.rs                    wiring and shutdown
  app/tools.rs              the tool layer both transports call
  app/server/mcp.rs         MCP over stdio (rmcp)
  app/server/http.rs        optional HTTP transport
  app/memory/
    model.rs                validated domain types
    db.rs, migrations/      SQLite connection, schema, tree helpers
    notes.rs                notes, links, full-text search, checkpoints
    entities.rs             entities, checks, claims, structured queries
    concepts.rs             LanceDB and embeddings
skills/pentacore-memory/    usage guide for agents
```

The SQLite schema is versioned through `PRAGMA user_version`; a database from an older release is upgraded in place on start.

## License

[MIT](LICENSE)
