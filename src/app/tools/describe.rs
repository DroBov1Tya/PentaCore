// What each tool is and takes, as the agent sees it. Fields that mean the
// same in every tool (author, run, task, confidence, basis) are explained
// once in the server instructions and carry only a type here.

use serde_json::{Value, json};
use std::fmt;

use super::{DEFAULT_GRAPH_DEPTH, Tool};
use crate::app::memory::entities::Op;
use crate::app::memory::journal::RecordType;
use crate::app::memory::model::{EdgeKind, MAX_GRAPH_DEPTH, NoteKind, Status};
use crate::app::memory::tasks;

// Every tool, for the HTTP API.
pub fn definitions() -> Value {
    let tools: Vec<Value> = Tool::ALL
        .iter()
        .map(|tool| {
            let (description, input_schema) = describe(*tool);
            json!({ "name": tool.as_str(), "description": description, "inputSchema": input_schema })
        })
        .collect();
    json!({ "tools": tools })
}

fn object(properties: Value, required: &[&str]) -> Value {
    json!({
        "type": "object",
        "properties": properties,
        "required": required,
        "additionalProperties": false,
    })
}

fn one_of<T: fmt::Display>(values: &[T]) -> Value {
    let names: Vec<String> = values.iter().map(T::to_string).collect();
    json!({ "type": "string", "enum": names })
}

fn text(description: &str) -> Value {
    json!({ "type": "string", "description": description })
}

fn id(description: &str) -> Value {
    json!({ "type": "integer", "description": description })
}

fn string() -> Value {
    json!({ "type": "string" })
}

fn integer() -> Value {
    json!({ "type": "integer" })
}

fn boolean() -> Value {
    json!({ "type": "boolean", "default": false })
}

fn confidence() -> Value {
    json!({ "type": "number", "minimum": 0, "maximum": 1 })
}

fn tags() -> Value {
    json!({ "type": "array", "items": { "type": "string" }, "maxItems": 16 })
}

fn ids() -> Value {
    json!({ "type": "array", "items": { "type": "integer" }, "maxItems": 64 })
}

fn token(description: &str) -> Value {
    json!({ "type": "string", "pattern": "^[a-z0-9][a-z0-9_.-]{0,47}$", "description": description })
}

fn limit(default: usize) -> Value {
    json!({ "type": "integer", "minimum": 1, "maximum": 100, "default": default })
}

fn depth() -> Value {
    json!({ "type": "integer", "minimum": 1, "maximum": MAX_GRAPH_DEPTH, "default": DEFAULT_GRAPH_DEPTH })
}

fn budget_chars() -> Value {
    json!({ "type": "integer", "minimum": tasks::MIN_BUDGET_CHARS,
        "maximum": tasks::MAX_BUDGET_CHARS, "default": tasks::DEFAULT_BUDGET_CHARS })
}

const FIELD_RULES: &str = "A column (id, type, key, status, confidence, parent, author, \
created_at, updated_at), `attrs.<name>`, `check.<name>` (null when never run) or `claimed`.";

const STATUS_RULES: &str = "goal/step: open, active, done, failed, dropped. attempt: active, \
done, failed (required). question: open, done, dropped. fact/decision/lesson: active, dropped.";

const PROJECT: &str = "'*' means all projects. Defaults to the current project.";

pub fn describe(tool: Tool) -> (&'static str, Value) {
    match tool {
        Tool::Note => (
            "Record a note: a goal, a step towards it, an attempt and its outcome, a fact, a \
             decision, an open question, or a lesson worth reusing. `parent` places it in the \
             tree. `similar` in the reply lists existing notes with the same title words.",
            object(
                json!({
                    "kind": one_of(NoteKind::ALL),
                    "title": text("One line, up to 200 characters."),
                    "body": text("The specifics: exact paths, commands, error texts, values, reasoning."),
                    "status": { "type": "string", "enum": status_names(), "description": STATUS_RULES },
                    "tags": tags(),
                    "parent": id("Note this one belongs under. A goal has none."),
                    "project": string(),
                    "links": {
                        "type": "array",
                        "maxItems": 16,
                        "items": object(json!({ "kind": one_of(EdgeKind::ALL), "to": integer() }), &["kind", "to"]),
                    },
                    "entity": id("Entity this note is about."),
                    "confidence": confidence(),
                    "basis": string(),
                    "author": string(),
                    "run": string(),
                    "task": integer(),
                }),
                &["kind", "title"],
            ),
        ),
        Tool::UpdateNote => (
            "Change a note. Omitted fields stay. Each change is a new revision; a call that \
             changes nothing makes none.",
            object(
                json!({
                    "id": integer(),
                    "title": string(),
                    "body": text("Replaces the old body."),
                    "append": text("Added to the end of the body, instead of `body`."),
                    "status": { "type": "string", "enum": status_names() },
                    "tags": tags(),
                    "parent": integer(),
                    "entity": integer(),
                    "confidence": confidence(),
                    "basis": string(),
                    "author": string(),
                    "run": string(),
                    "expected_revision": id("The revision you read; a conflict is returned if it changed since."),
                }),
                &["id"],
            ),
        ),
        Tool::GetNote => (
            "One note in full, with its confidence, children, links and checklist.",
            object(json!({ "id": integer() }), &["id"]),
        ),
        Tool::Recall => (
            "Search notes and entities, most relevant first; `source` tells them apart. Each \
             note comes with `confidence` and `basis` when its author gave them. Dropped or \
             superseded notes rank lower and name their replacement in `superseded_by`. Notes \
             saying the same thing are shown once, the others under `duplicates`.",
            object(
                json!({
                    "query": text("What to look for."),
                    "project": text(PROJECT),
                    "task": id("Task being worked on; its notes rank a little higher."),
                    "kind": { "type": "string", "enum": kind_names(),
                        "description": "Only notes of this kind, e.g. 'lesson' for stored know-how." },
                    "limit": limit(8),
                }),
                &["query"],
            ),
        ),
        Tool::Link => (
            "Connect two notes, read as 'src <kind> dst'; remove=true takes the link away. \
             depends_on must stay acyclic.",
            object(
                json!({
                    "src": integer(),
                    "dst": integer(),
                    "kind": one_of(EdgeKind::ALL),
                    "remove": boolean(),
                }),
                &["src", "dst", "kind"],
            ),
        ),
        Tool::Graph => (
            "The tree beneath a note or an entity, depth first, with every link touching it. \
             Bodies are omitted.",
            object(
                json!({
                    "type": one_of(&[RecordType::Note, RecordType::Entity]),
                    "id": integer(),
                    "depth": depth(),
                }),
                &["type", "id"],
            ),
        ),
        Tool::Resume => (
            "Without `task`: unfinished goals, steps in progress, open questions, recent notes, \
             entity counts, the last checkpoint, and `tasks` with the changes since each one's \
             checkpoint. With `task`: its last checkpoint, progress, blockers, failed attempts, \
             changes after the checkpoint, next steps, decisions, and every checklist item \
             still open, cut to `budget_chars`.",
            object(
                json!({
                    "project": text(PROJECT),
                    "task": id("Goal or step to continue. Not together with `project`."),
                    "budget_chars": budget_chars(),
                }),
                &[],
            ),
        ),
        Tool::Checkpoint => (
            "Save where the work stands mid-way, to be read back by `resume`. With `task` it \
             belongs to that task. To close a task use `summarize` instead.",
            object(
                json!({
                    "summary": text("What is done, what is in progress, what comes next, and why."),
                    "task": integer(),
                    "project": string(),
                    "author": string(),
                    "run": string(),
                }),
                &["summary"],
            ),
        ),
        Tool::Checklist => (
            "The plan of a step as items that are done or not done. One call can add, tick, \
             untick and remove; with only `note` it reads. Returns the whole list.",
            object(
                json!({
                    "note": integer(),
                    "add": { "type": "array", "maxItems": 32, "items": { "type": "string" },
                        "description": "New items, one line each." },
                    "done": ids(),
                    "undone": ids(),
                    "remove": ids(),
                    "author": string(),
                    "run": string(),
                }),
                &["note"],
            ),
        ),
        Tool::Summarize => (
            "Close a goal or step: write its one summary and set its status. Calling again \
             replaces the summary; the old text stays as a revision. The reply names checklist \
             items left open and finished sub-tasks without a summary.",
            object(
                json!({
                    "task": integer(),
                    "result": text("The outcome in detail, usable without the task's notes."),
                    "work": text("All that was done, in brief, failures included."),
                    "status": { "type": "string", "enum": ["done", "failed", "dropped"], "default": "done" },
                    "confidence": confidence(),
                    "basis": string(),
                    "author": string(),
                    "run": string(),
                }),
                &["task", "result", "work"],
            ),
        ),
        Tool::TaskSummary => (
            "Read a task from summaries alone: its own, then those of the goals and steps \
             beneath it, plus checklist items still open. Answers 'what was done and found \
             here' without the task's notes.",
            object(
                json!({ "task": integer(), "budget_chars": budget_chars() }),
                &["task"],
            ),
        ),
        Tool::Confirm => (
            "Record that a note was checked again now and still holds. Sets `verified_at`; a \
             new `confidence` or `basis` is kept as a revision. If it proved wrong, correct it \
             with `update_note` instead.",
            object(
                json!({
                    "id": integer(),
                    "confidence": confidence(),
                    "basis": string(),
                    "author": string(),
                    "run": string(),
                }),
                &["id"],
            ),
        ),
        Tool::UpsertEntity => (
            "Create or update a record of one of many similar things, identified by (project, \
             type, key). Only the fields given change; `attrs` are merged, null removes one. \
             With just type and key it returns the entity unchanged.",
            object(
                json!({
                    "type": token("Kind of thing, e.g. 'host', 'endpoint'."),
                    "key": text("Its name within the type."),
                    "project": string(),
                    "status": token("Free-form state. Defaults to 'new'."),
                    "confidence": confidence(),
                    "parent": integer(),
                    "attrs": {
                        "type": "object",
                        "additionalProperties": { "type": ["string", "number", "boolean", "null"] },
                    },
                    "author": string(),
                    "run": string(),
                    "task": integer(),
                    "claim_id": string(),
                }),
                &["type", "key"],
            ),
        ),
        Tool::QueryEntities => (
            "Filter entities by field; conditions are combined with AND. `total` counts every \
             match. With `group_by`, counts per value are returned instead of entities.",
            object(
                json!({
                    "project": text(PROJECT),
                    "type": string(),
                    "where": {
                        "type": "array",
                        "maxItems": 16,
                        "items": object(
                            json!({
                                "field": text(FIELD_RULES),
                                "op": one_of(Op::ALL),
                                "value": { "description": "Scalar; an array for 'in'; omitted for is_null and not_null." },
                            }),
                            &["field", "op"],
                        ),
                    },
                    "order_by": string(),
                    "descending": boolean(),
                    "group_by": string(),
                    "limit": limit(20),
                }),
                &[],
            ),
        ),
        Tool::MarkCheck => (
            "Record the result of a named check on an entity; marking it again replaces it. \
             Queries read it as `check.<name>`. Give one check, or several as `checks`.",
            object(
                json!({
                    "id": integer(),
                    "name": string(),
                    "result": token("E.g. 'pass', 'fail', 'skipped'."),
                    "detail": text("One line."),
                    "checks": {
                        "type": "array",
                        "maxItems": 32,
                        "items": object(
                            json!({ "name": string(), "result": string(), "detail": string() }),
                            &["name", "result"],
                        ),
                    },
                    "author": string(),
                    "run": string(),
                    "task": integer(),
                    "claim_id": string(),
                }),
                &["id"],
            ),
        ),
        Tool::Forget => (
            "Delete a note or an entity permanently with all revisions, links, checklist and \
             events; only a journal entry that it existed remains. One with children needs \
             recursive=true. For what was once true, set a note to `dropped` instead.",
            object(
                json!({
                    "type": one_of(&[RecordType::Note, RecordType::Entity]),
                    "id": integer(),
                    "recursive": boolean(),
                    "claim_id": string(),
                    "author": string(),
                    "run": string(),
                }),
                &["type", "id"],
            ),
        ),
        Tool::Find => (
            "Search notes by exact words, with filters. `matched` is 'all', 'prefix' or 'any'.",
            object(
                json!({
                    "query": string(),
                    "project": text(PROJECT),
                    "kind": { "type": "string", "enum": kind_names() },
                    "status": { "type": "string", "enum": status_names() },
                    "under": id("Only this note and everything beneath it."),
                    "entity": integer(),
                    "limit": limit(10),
                }),
                &["query"],
            ),
        ),
        Tool::GetEntity => (
            "One entity in full: checks, children, links, notes about it, its 20 latest events.",
            object(json!({ "id": integer() }), &["id"]),
        ),
        Tool::ClaimEntity => (
            "Reserve an entity: until the claim expires, only calls with the returned \
             `claim_id` can change it. Presenting the claim_id renews it; release=true gives \
             it up.",
            object(
                json!({
                    "id": integer(),
                    "ttl_seconds": { "type": "integer", "minimum": 1, "maximum": 86400, "default": 900 },
                    "claim_id": string(),
                    "release": boolean(),
                    "author": string(),
                    "run": string(),
                }),
                &["id"],
            ),
        ),
        Tool::LinkEntities => (
            "Connect two entities with a named link, 'src <kind> dst'; remove=true takes it away.",
            object(
                json!({
                    "src": integer(),
                    "dst": integer(),
                    "kind": string(),
                    "remove": boolean(),
                }),
                &["src", "dst", "kind"],
            ),
        ),
        Tool::History => (
            "List changes, newest first: record, `op`, changed `fields`, author, run, task, \
             time. Give `type` and `id` for one record, `task` for one task, or neither for the \
             project. `origin` 'legacy_baseline' is the state when history began. Pass \
             `next_cursor` as `cursor` for the next page.",
            object(
                json!({
                    "type": one_of(RecordType::ALL),
                    "id": integer(),
                    "project": text(PROJECT),
                    "task": integer(),
                    "cursor": integer(),
                    "limit": limit(20),
                }),
                &[],
            ),
        ),
        Tool::GetRevision => (
            "The state a note had at an earlier revision. The 50 latest are kept.",
            object(json!({ "id": integer(), "revision": integer() }), &["id"]),
        ),
        Tool::RedactHistory => (
            "Permanently delete every earlier revision of a note, keeping its current state. \
             For a secret that was edited out: editing alone leaves it in the revisions.",
            object(
                json!({ "id": integer(), "author": string(), "run": string() }),
                &["id"],
            ),
        ),
        Tool::Review => (
            "What in a project needs attention: stalled tasks, open questions, tasks closed \
             without a summary, open checklist items, loose notes, low-confidence notes, \
             lessons not confirmed lately, claims. Read-only.",
            object(
                json!({
                    "project": text(PROJECT),
                    "stale_days": { "type": "integer", "minimum": 1, "maximum": 365, "default": 14 },
                }),
                &[],
            ),
        ),
    }
}

fn kind_names() -> Vec<&'static str> {
    NoteKind::ALL.iter().map(|kind| kind.as_str()).collect()
}

fn status_names() -> Vec<&'static str> {
    Status::ALL.iter().map(|status| status.as_str()).collect()
}
