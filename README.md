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
| **Concepts** | Generalised, reusable knowledge | Meaning (`recall`) | SQLite; vectors in LanceDB |

SQLite holds every record. LanceDB holds only vectors of notes and concepts, for search by meaning; it can be deleted and is rebuilt from SQLite.

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

The first search by meaning (`recall`, `memorize_concept`) downloads the embedding model (all-MiniLM-L6-v2, about 90 MB) into the data directory. Everything works without it, by words.

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
| `PENTACORE_HTTP_ADDR` | unset (HTTP off) | Loopback address and port for the HTTP API. |
| `PENTACORE_HTTP_TOKEN` | unset | Bearer token for the HTTP API, at least 32 characters. Required when the address is set. |
| `RUST_LOG` | `warn,pentacore=info` | Log filter. Logs go to stderr. |

Variables can also come from a `.env` file **beside the executable**; see [`bin/.env.example`](bin/.env.example). A `.env` in the working directory is ignored on purpose: the working directory may be a checkout you do not trust.

## Tools

**Notes**

| Tool | Does |
|---|---|
| `note` | Record a goal, step, attempt, fact, decision or question. Reports similar existing notes. |
| `update_note` | Change fields, move under another parent, or `append` to the body. Each change is a new revision; `expected_revision` refuses a write over someone else's change. |
| `get_note` | One note in full, with children and links. |
| `find` | Full-text search. Falls back from exact words to word prefixes to any word. |
| `link` | Typed links: `depends_on`, `supports`, `contradicts`, `answers`, `supersedes`, `relates_to`. `remove: true` takes one away. |

**Entities**

| Tool | Does |
|---|---|
| `upsert_entity` | Create or update by `(project, type, key)`. Merges attributes; an identical write changes nothing. |
| `query_entities` | Filter, sort, count and group by column, `attrs.<name>` or `check.<name>`. |
| `mark_check` | Record the result of a named check. |
| `claim_entity` | Reserve an entity for one agent, with a timeout; `release: true` gives it up. |
| `link_entities` | Named links between entities. |
| `get_entity` | One entity with checks, children, links, notes about it and its 20 latest events. |

**Concepts**

| Tool | Does |
|---|---|
| `memorize_concept` | Store a concept. Reports existing ones that say nearly the same. |
| `update_concept` | Change a concept, or archive and restore it. Each change is a new revision. |

**Across stores**

| Tool | Does |
|---|---|
| `recall` | Search notes, entities and concepts in one call, ranked by relevance, repeats shown once. `only` narrows it to one store. |
| `graph` | The tree beneath a note or an entity, with every link touching it. |
| `forget` | Purge a note, an entity or a concept with every revision; a subtree with `recursive`. |
| `checkpoint` | Save a summary of where the work stands, for the project or for one `task`. |
| `resume` | Current state of a project: open goals, active steps, open questions, recent notes, entity counts, last checkpoint, unfinished tasks. With `task`: what is needed to continue that task, within a size budget. |

**History**

| Tool | Does |
|---|---|
| `history` | Who changed what and when, newest first, for a record, a task or a project. Paged by cursor. |
| `get_revision` | The state a note or concept had at an earlier revision. |
| `redact_history` | Purge the earlier revisions of a note or concept, keeping its current state. |

**Working practices**

| Tool | Does |
|---|---|
| `checklist` | Items on a note that are done or not done. Add, tick, untick, remove, or read. |
| `summarize` | Close a goal or step with its one summary: all that was done in brief, the outcome in detail. |
| `task_summary` | Read a task from its summaries alone, its own and those of the goals and steps beneath it. |
| `confirm` | Record that a note or concept was checked again and still holds, with confidence and basis. |
| `review` | What in a project needs attention: stalled tasks, open questions, tasks closed without a summary, open checklist items, doubtful or long-unchecked records. |

## Working practices

The memory is meant to be worked in a particular way, and the tools are shaped for it.

**A tree, not a pile.** A task is a goal; what it breaks into are steps beneath it; findings sit beneath the step they belong to. Checking `example.com` is a goal, each subdomain a step.

**Done or not done.** A step carries a `checklist`. An item has two states only. Whoever picks the task up later, another model or another session, reads what is left without interpreting anyone's status vocabulary. `resume` for a task returns every unticked item beneath it.

**A summary at every level.** When a step is finished, `summarize` writes its one summary while the whole context is still in view: `work`, everything that was done, in brief, failures included; `result`, the outcome in detail. When the goal is finished it gets its own, written from the summaries of its steps. `task_summary` then returns a task as its summaries in tree order and nothing else, so the question "what was checked on `example.com`, and what came of it" costs one call however many notes lie beneath. A summary is a note under its task: it is searched, revised and journaled like any other, and a rewrite keeps the earlier text as a revision.

**How far to trust a record.** `note`, `update_note`, `memorize_concept` and `update_concept` take `confidence` (0 to 1) and `basis`, one or two sentences on what the confidence rests on: long-established public fact, reproduced here, one unverified source, an inference. `confirm` marks a record as checked again and found true, setting `verified_at` without touching the text. `recall` shows `confidence` and `verified_at` on each result. Confidence and basis are kept beside the record, not inside its revisions: the journal records that they changed, not their earlier values.

**Know-how in concepts.** A concept is written as a finding someone else can reuse: the problem, what was found and where, how it was checked, whether it was confirmed or refuted, what was done. When a later, better check reaches a different conclusion, the concept is updated (the earlier text stays as a revision) or archived, and searches return only the current text.

**Search before experimenting.** The tool instructions tell the agent to look for earlier work first and to experiment only where nothing is recorded.

## History and provenance

Every change is written to a journal in the same transaction as the change: the record, the kind of change, the names of the fields, and the `author`, `run` and `task` the caller gave. These are labels, not credentials. The journal never holds the text of a record.

Notes and concepts also keep their full state after each change, as numbered revisions. Entities keep their events, with old and new values.

What existed before this version has no recorded history, and none is invented. On upgrade, the state of each note (and of each concept, when it is first read from the old vector store) becomes revision 1 with origin `legacy_baseline`. Entity events and checkpoints that were already logged are copied into the journal with origin `backfilled`. New entries have origin `recorded`.

## Keeping, archiving and purging

| | What it does | What remains |
|---|---|---|
| **Drop / archive** | `status: dropped` on a note, `archived: true` on a concept. For what was once true. | Everything. A dropped note ranks lower in `recall`; an archived concept is left out of searches unless asked for. Reversible. |
| **Retention** | Automatic. The 50 latest revisions of a record and the 20 latest checkpoints per task (and per project) are kept. | Journal entries of the dropped revisions, without their text. |
| **Redact** | `redact_history`: removes all earlier revisions of one record. For a secret that was edited out. | The current state and the journal entries. |
| **Purge** | `forget`: removes the record, its revisions, events, search-index rows, vector, and a task's checkpoints. For what should never have been stored. | One journal entry per record: its id, kind and project, who purged it and when. No title, no text. |

"Removes" here means from SQLite, its full-text indexes and its write-ahead log. Copies in LanceDB's older table versions, backups and filesystem snapshots are outside its reach; see the known limits under Security.

Nothing is purged automatically, and `recall` never deletes what it folds as a repeat. Journal entries are never removed; they hold identifiers, field names (including attribute and check names) and labels, so do not put secrets into those.

## Continuing a task

A task is a note, usually a goal, with everything beneath it. `checkpoint` with `task` saves a summary for that task alone, so several tasks in one project do not overwrite each other. `resume` with `task` returns, in this order and within `budget_chars` (8000 by default): the task's last checkpoint, the task itself, step counts, blockers (open questions, steps waiting on unfinished `depends_on` targets, entities someone holds a claim on), steps in progress, failed attempts, the changes journaled after the checkpoint, the next open steps and the decisions in force. `budget.omitted` counts what did not fit per section.

## Search quality

`recall` scores each candidate by how much of the query its words cover (rare words weigh more; another form of a word counts for less than the exact word) and, when the model is loaded, by closeness in meaning. Either is enough; agreement adds a little. A dropped, archived or superseded record is scaled down. Freshness adds at most 6% and belonging to the given `task` 20%, so neither lifts an irrelevant record over a relevant one.

The tool descriptions and the usage guide tell agents to write every record and every query in English, in ASCII (stored as UTF-8). This is guidance, not validation: other text is still accepted, so existing records keep working. It is what makes search by meaning apply to the whole memory.

Records are folded as repeats when their words nearly coincide, or, with the model, when they are close in meaning and share a good part of their words.

The ranking is measured on a judged corpus of 102 records and 70 queries in Russian and English; see [`eval/README.md`](eval/README.md) for the numbers, how to reproduce them and what they do not show. In short: the embedding model is English-only, so search by meaning is used for text in the Latin alphabet only, and Russian rewordings and cross-language queries are found by shared words or not at all.

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
- LanceDB never rewrites its files here: a delete hides a row, and the earlier table versions stay on disk. The new index holds vectors only, no text, but the vector of a purged record remains in those old versions. The `concepts` table written by releases before 0.6 holds text: it is read once at the first start of this release and left in place; a concept purged or redacted later is hidden in it (purge) or untouched in it (redact), and stays recoverable from the files either way. To remove it for good, start this release once, check the log for `concepts moved from the vector store into SQLite` (or that `recall` with `only: "concept"` returns them), stop the server and delete `concepts.lancedb`; the index is rebuilt. Deleting it before that first start loses the old concepts.
- Purge and redact empty SQLite's write-ahead log afterwards, so the removed text does not linger there. If another process holds the database open for reading at that moment, the log is emptied at a later purge instead; a warning is logged.
- `redact_history` covers the revisions of notes and concepts only. Earlier attribute values of an entity stay in its events until the entity is purged, and a checkpoint saved without a task leaves only when 20 newer ones have replaced it.
- The journal and LanceDB's files grow without bound; nothing compacts them yet.
- `author`, `run` and `task` are unverified labels. The journal shows what callers claimed, not who they were.
- Records are embedded in the background after a search has loaded the model, so a record written a moment ago may be found by words only; `recall` reports the backlog as `index_pending`.
- Claims prevent accidental collisions between cooperating agents. They do not stop a hostile process that can open the database file.

## Development

```sh
cargo test                       # unit and integration tests
cargo clippy --all-targets -- -D warnings
cargo fmt --check
cargo audit
```

Tests that need the embedding model are skipped by default. Point them at a fastembed cache, or let them download the model:

```sh
PENTACORE_TEST_MODELS=~/.fastembed_cache cargo test --release -- --include-ignored
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
    concepts.rs             concepts, import from the old vector store
    journal.rs              journal, revisions, history, redaction
    tasks.rs                resuming one task within a budget
    practices.rs            checklists, task summaries, confidence, review
    words.rs                matching and scoring by words
    index.rs                embeddings and the vector index
    recall.rs               ranking across stores, folding repeats
  **/tests/                 unit tests, one file per module; app/tests/eval.rs is the retrieval benchmark
eval/                       judged corpus, results, how to read them
skills/pentacore-memory/    usage guide for agents
```

The SQLite schema is versioned through `PRAGMA user_version`; a database from an older release is upgraded in place on start, in one transaction. A release older than this one cannot open the upgraded database, so copy `brain.sqlite` before the first start if you may need to go back.

## License

[MIT](LICENSE)
