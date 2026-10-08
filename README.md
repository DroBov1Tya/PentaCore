# pentacore

A quest log for AI agents, served over the [Model Context Protocol](https://modelcontextprotocol.io).

An agent without memory goes in circles: it retries what already failed, loses its plan when the context is compacted, and cannot say what was done last week. pentacore is where it writes down what it set out to do, what it tried, what it found and what is left, and where the next session, or the next model, picks that up.

It is a tool, not a manager. It stores what it is given and returns what is asked for.

- One local binary, one SQLite file. No services to run.
- Built on the official Rust MCP SDK ([`rmcp`](https://github.com/modelcontextprotocol/rust-sdk)).
- Shared by any number of agents and projects at once.

## What it stores

| Store | Holds | Found by |
|---|---|---|
| **Notes** | Goals, steps, attempts and their outcomes, facts, decisions, open questions, lessons | Words and meaning (`recall`) |
| **Entities** | Many things of one kind (hosts, endpoints, files), each with a status, attributes and named checks | Field filters (`query_entities`) |

Notes form a tree through `parent` and a graph through typed links, which is how a goal is broken down and tracked. A note can point at the entity it is about.

The split matters. "Show every host whose `tls` check failed" is a query over fields. It should not depend on how somebody phrased a note.

## How it is meant to be used

**A tree, not a pile.** A task is a goal; its parts are steps beneath it; findings sit beneath the step they belong to. Checking `example.com` is a goal, each subdomain a step.

**Done or not done.** A step carries a `checklist`. An item has two states only, so whoever picks the task up later reads what is left without interpreting anyone's status words. For many similar targets, each target is an entity and each check a `mark_check`, so that "which hosts are left" is one query.

**A summary at every level.** When a step is finished, `summarize` writes its one summary and closes it: `work`, everything that was done, in brief, failures included; `result`, the outcome in detail. A goal gets its own, written from the summaries of its steps. `task_summary` returns a task as its summaries in tree order and nothing else, so "what was checked on `example.com`, and what came of it" costs one call however many notes lie beneath.

**How far to trust a note.** A note carries `confidence` (0 to 1) and `basis`, a sentence or two on what the confidence rests on. They come back with the text wherever the note is read, so the next agent knows how sure the last one was. `confirm` records that a note was checked again and found true; that is the only thing that sets `verified_at`. Confidence does not affect ranking.

**Lessons.** Know-how worth reusing is a note of kind `lesson`: the problem, what was found, how it was checked, whether it was confirmed, what was done. When a later, better check reaches a different conclusion, the lesson is updated; the earlier text stays as a revision and searches return only the current one.

**Search before experimenting.** The instructions the server sends tell the agent to look for earlier work first.

## Install

Requires Rust 1.88 or newer.

```sh
git clone <this repository>
cd pentacore
cargo build --release
```

The binary is `target/release/pentacore`. Copy it wherever you keep tools.

The first `recall` downloads the embedding model (all-MiniLM-L6-v2, about 90 MB) into the data directory. Everything works without it, by words.

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
| `PENTACORE_HOME` | `~/.pentacore` | Data directory. `~/` means the user's home. Any other relative path is taken from the executable's directory, not the working directory. Created with mode `0700`. |
| `PENTACORE_PROJECT` | name of the working directory | Project used when a call names none. |
| `PENTACORE_TOOLS` | `core` | `core` lists the 16 everyday tools to the agent; `full` lists all 24. Every tool can be called either way. |
| `PENTACORE_HTTP_ADDR` | unset (HTTP off) | Loopback address and port for the HTTP API. |
| `PENTACORE_HTTP_TOKEN` | unset | Bearer token for the HTTP API, at least 32 characters. Required when the address is set. |
| `RUST_LOG` | `warn,pentacore=info` | Log filter. Logs go to stderr. |

Variables can also come from a `.env` file **beside the executable**; see [`bin/.env.example`](bin/.env.example). A `.env` in the working directory is ignored on purpose: the working directory may be a checkout you do not trust.

## Tools

Listed by default (`core`):

| Tool | Does |
|---|---|
| `note` | Record a goal, step, attempt, fact, decision, question or lesson, with confidence and basis. |
| `update_note` | Change a note. Each change is a new revision; `expected_revision` refuses a write over someone else's change. |
| `get_note` | One note in full, with children, links and checklist. |
| `recall` | Search notes and entities, most relevant first, repeats shown once. `kind` narrows it, e.g. to lessons. |
| `link` | Typed links between notes: `depends_on`, `supports`, `contradicts`, `answers`, `supersedes`, `relates_to`. |
| `graph` | The tree beneath a note or an entity. |
| `resume` | State of a project, or with `task` what is needed to continue that task, within a size budget. |
| `checkpoint` | Save where the work stands mid-way, for the project or one task. |
| `checklist` | Items on a note that are done or not done. |
| `summarize` | Close a goal or step with its one summary. |
| `task_summary` | Read a task from its summaries alone. |
| `confirm` | Record that a note was checked again and still holds. |
| `upsert_entity` | Create or update by `(project, type, key)`; attributes are merged. |
| `query_entities` | Filter, sort, count and group by column, `attrs.<name>` or `check.<name>`. |
| `mark_check` | Record the result of one or several named checks on an entity. |
| `forget` | Purge a note or an entity with every revision. |

Listed with `PENTACORE_TOOLS=full`:

| Tool | Does |
|---|---|
| `find` | Search notes by exact words, with filters. |
| `get_entity` | One entity with checks, children, links, notes about it and its latest events. |
| `claim_entity` | Reserve an entity for one agent, with a timeout; `release: true` gives it up. |
| `link_entities` | Named links between entities. |
| `history` | Who changed what and when, for a record, a task or a project. Paged by cursor. |
| `get_revision` | The state a note had at an earlier revision. |
| `redact_history` | Purge the earlier revisions of a note, keeping its current state. |
| `review` | What needs attention: stalled tasks, open questions, tasks closed without a summary, open checklist items, low-confidence notes. |

Each tool carries a JSON Schema and MCP annotations (`readOnlyHint`, `destructiveHint`).

## History

Every change is written to a journal in the same transaction: the record, the kind of change, the names of the fields, and the `author`, `run` and `task` the caller gave. These are labels, not credentials. The journal never holds the text of a record.

Notes keep their full state after each change, confidence and basis included, as numbered revisions. Entities keep their events, with old and new values.

What existed before 0.7 has no recorded history, and none is invented. On upgrade, the state of each note becomes revision 1 with origin `legacy_baseline`; entity events and checkpoints already logged are copied into the journal with origin `backfilled`.

## Keeping and purging

| | What it does | What remains |
|---|---|---|
| **Drop** | `status: dropped` on a note. For what was once true. | Everything; the note ranks lower in `recall`. Reversible. |
| **Retention** | Automatic. The 50 latest revisions of a note and the 20 latest checkpoints per task (and per project) are kept. | Journal entries of the dropped revisions, without text. |
| **Redact** | `redact_history`: removes all earlier revisions of one note. For a secret that was edited out. | The current state and the journal entries. |
| **Purge** | `forget`: removes the record, its revisions, checklist, events, vector, and a task's checkpoints. For what should never have been stored. | One journal entry per record: its id, kind and project, who purged it and when. |

"Removes" means from the SQLite file and its write-ahead log, which is emptied after a purge or redaction. Backups and filesystem snapshots are outside its reach. Nothing is purged automatically.

## Search

`recall` scores each candidate by how much of the query its words cover (rare words weigh more; another form of a word counts for less than the exact word) and, when the model is loaded, by closeness in meaning. Either is enough; agreement adds a little. A dropped or superseded note is scaled down. Freshness adds at most 6% and belonging to the given `task` 20%.

Notes are folded as repeats when their words nearly coincide, or, with the model, when they are close in meaning and share a good part of their words.

The vector of a note is stored beside it in SQLite and compared by a full scan, which is fast for tens of thousands of notes and has not been measured beyond that. The embedding model is English-only, so search by meaning is used for text in the Latin alphabet only; the instructions tell agents to write records and queries in English, in ASCII. That is guidance, not validation.

## Example

```json
{"name": "note", "arguments": {"kind": "goal", "title": "Check example.com"}}
{"name": "note", "arguments": {"kind": "step", "title": "Check a.example.com", "parent": 1}}
{"name": "checklist", "arguments": {"note": 2, "add": ["TLS configuration", "Open ports"]}}
{"name": "note", "arguments": {"kind": "fact", "title": "TLS 1.0 enabled on a.example.com", "parent": 2,
  "body": "Handshake accepts TLSv1.0 on port 443.", "confidence": 0.9, "basis": "two independent scans"}}
{"name": "checklist", "arguments": {"note": 2, "done": [1, 2]}}
{"name": "summarize", "arguments": {"task": 2, "result": "TLS 1.0 is enabled on port 443; no other ports open.",
  "work": "TLS and ports checked."}}
```

In a later session:

```json
{"name": "resume"}
{"name": "task_summary", "arguments": {"task": 1}}
{"name": "recall", "arguments": {"query": "TLS 1.0"}}
```

## Projects and several agents

Everything belongs to a project. Reads default to the current project, and `"project": "*"` reads across all of them, so a lesson learned in one place can be found from another.

Several agents can write to the same project at the same time:

- **Claims.** `claim_entity` returns a secret `claim_id`. Until the claim expires or is released, only calls that present it can change the entity. Of two simultaneous claims exactly one wins.
- **Revisions.** `update_note` with `expected_revision` is refused if the note changed in between.

A project is a namespace, not a security boundary: any agent connected to the same data directory can read and change any project. Use separate `PENTACORE_HOME` directories for memories that must not mix.

## HTTP API

Off by default. Set both `PENTACORE_HTTP_ADDR` and `PENTACORE_HTTP_TOKEN` to turn it on. It exposes every tool:

```sh
curl -s http://127.0.0.1:8082/v1/tools/recall \
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
- **Data at rest** sits in a `0700` directory. Deleted rows are overwritten (`secure_delete`), and the write-ahead log is emptied after a purge.

Known limits:

- Stored text is not sanitised. An agent that saves untrusted text will read it back later; the usage guide tells agents to treat recalled memory as data, not instructions.
- `author`, `run` and `task` are unverified labels, and so are `confidence` and `basis`: any caller can set them. Their earlier values stay in the revisions.
- `redact_history` covers note revisions only. Earlier attribute values of an entity stay in its events until the entity is purged, and a checkpoint saved without a task leaves only when 20 newer ones have replaced it.
- If another process holds the database open for reading during a purge, the write-ahead log is emptied at a later purge instead; a warning is logged.
- The journal grows without bound.
- The HTTP API has no rate limiting and no header-read timeout.
- Claims prevent accidental collisions between cooperating agents. They do not stop a hostile process that can open the database file.
- Concepts kept in LanceDB by releases up to 0.5 are not migrated; 0.7 does not read that directory.

## Development

```sh
cargo clippy --all-targets -- -D warnings
cargo fmt --check
cargo audit
```

Layout:

```
src/
  main.rs, config.rs        entry point and environment
  app.rs                    wiring and shutdown
  app/tools.rs              the tool layer both transports call
  app/tools/describe.rs     tool descriptions and JSON Schemas
  app/server/mcp.rs         MCP over stdio (rmcp)
  app/server/http.rs        optional HTTP transport
  app/memory/
    model.rs                validated domain types
    db.rs, migrations/      SQLite connection, schema, tree helpers
    notes.rs                notes, links, full-text search, checkpoints
    entities.rs             entities, checks, claims, structured queries
    journal.rs              journal, revisions, history, redaction
    tasks.rs                resuming one task within a budget
    practices.rs            checklists, task summaries, confirm, review
    words.rs                matching and scoring by words
    index.rs                embeddings and search by meaning
    recall.rs               ranking, folding repeats
skills/pentacore-memory/    usage guide for agents
```

The SQLite schema is versioned through `PRAGMA user_version`; a database from an older release is upgraded in place on start, in one transaction. A release older than this one cannot open the upgraded database, so copy `brain.sqlite` before the first start if you may need to go back.

## License

[MIT](LICENSE)
