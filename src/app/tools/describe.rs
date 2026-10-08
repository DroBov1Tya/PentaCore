// What each tool is and takes, as the agent sees it.

use serde_json::{Value, json};
use std::fmt;

use super::{DEFAULT_GRAPH_DEPTH, Tool};
use crate::app::memory::entities::Op;
use crate::app::memory::journal::RecordType;
use crate::app::memory::model::{EdgeKind, MAX_GRAPH_DEPTH, NoteKind, Status};
use crate::app::memory::tasks;

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

fn one_of<T: fmt::Display>(values: &[T], description: &str) -> Value {
    let names: Vec<String> = values.iter().map(T::to_string).collect();
    json!({ "type": "string", "enum": names, "description": description })
}

fn text(description: &str) -> Value {
    json!({ "type": "string", "description": description })
}

fn note_id(description: &str) -> Value {
    json!({ "type": "integer", "description": description })
}

fn tags() -> Value {
    json!({ "type": "array", "items": { "type": "string" }, "maxItems": 16,
            "description": "Short labels without spaces, in English, ASCII only." })
}

fn project(description: &str) -> Value {
    text(description)
}

fn entity_id(description: &str) -> Value {
    json!({ "type": "integer", "description": description })
}

fn author() -> Value {
    text("Who wrote this, e.g. 'claude/reviewer'. A label only.")
}

fn run_label() -> Value {
    text("Session or run id. A label only.")
}

fn task(description: &str) -> Value {
    json!({ "type": "integer", "description": description })
}

fn expected_revision() -> Value {
    json!({ "type": "integer", "description":
        "The revision you read; a conflict is returned if it changed since." })
}

fn record_id() -> Value {
    json!({ "type": ["integer", "string"], "description":
        "An integer for a note, entity or checkpoint; a UUID string for a concept." })
}

fn confidence() -> Value {
    json!({ "type": "number", "minimum": 0, "maximum": 1, "description":
        "How sure you are that this holds, 0 to 1. Give with `basis`." })
}

fn basis() -> Value {
    json!({ "type": "string", "maxLength": 500, "description":
        "What the confidence rests on, in a sentence or two." })
}

fn item_ids(description: &str) -> Value {
    json!({ "type": "array", "items": { "type": "integer" }, "maxItems": 64, "description": description })
}

fn budget_chars() -> Value {
    json!({ "type": "integer", "minimum": tasks::MIN_BUDGET_CHARS,
        "maximum": tasks::MAX_BUDGET_CHARS, "default": tasks::DEFAULT_BUDGET_CHARS,
        "description": "Size of the reply in characters." })
}

fn claim_id() -> Value {
    text("The claim_id from claim_entity; required while the entity is claimed.")
}

fn token(description: &str) -> Value {
    json!({ "type": "string", "pattern": "^[a-z0-9][a-z0-9_.-]{0,47}$", "description": description })
}

pub(super) const FIELD_RULES: &str = "A column (id, type, key, status, confidence, parent, author, \
created_at, updated_at), `attrs.<name>`, `check.<name>` (the result of that check, null when it \
was never run) or `claimed`.";

fn limit(default: usize) -> Value {
    json!({ "type": "integer", "minimum": 1, "maximum": 100, "default": default })
}

const STATUS_RULES: &str = "goal/step: open, active, done, failed, dropped. \
attempt: active, done, failed (required: the outcome). question: open, done, dropped. \
fact/decision: active (holds), dropped (no longer valid).";

const EDGE_RULES: &str = "Read as 'src <kind> dst'. depends_on must stay acyclic.";

pub fn describe(tool: Tool) -> (&'static str, Value) {
    match tool {
        Tool::Note => (
            "Record a note: a goal, a step towards it, an attempt and its outcome, a concrete \
             fact, a decision and its reason, or an open question. `parent` places it in the tree \
             under another note; `links` connects it to related notes. The reply lists `similar` \
             existing notes when their text contains every word of the title.",
            object(
                json!({
                    "kind": one_of(NoteKind::ALL, "What the note records."),
                    "title": text("One line, up to 200 characters. English, ASCII only."),
                    "body": text("The specifics: exact paths, commands, error texts, values, reasoning. English, ASCII only."),
                    "status": one_of(Status::ALL, STATUS_RULES),
                    "tags": tags(),
                    "parent": note_id("Id of the note this one belongs under. A goal has none."),
                    "project": project("Defaults to the parent's project, else the current project."),
                    "links": {
                        "type": "array",
                        "maxItems": 16,
                        "description": "Links from the new note to existing ones.",
                        "items": object(
                            json!({ "kind": one_of(EdgeKind::ALL, EDGE_RULES), "to": note_id("Target note id.") }),
                            &["kind", "to"],
                        ),
                    },
                    "entity": entity_id("Entity this note is about, in the same project."),
                    "author": author(),
                    "run": run_label(),
                    "task": task("Note id of the task this change belongs to. Defaults to the root of the note's tree."),
                    "confidence": confidence(),
                    "basis": basis(),
                }),
                &["kind", "title"],
            ),
        ),
        Tool::UpdateNote => (
            "Change a note's title, body, status, tags, parent or entity. Omitted fields stay as \
             they are. Each change makes a new revision; the earlier state stays readable through \
             `get_revision`. A call that changes nothing makes none.",
            object(
                json!({
                    "id": note_id("Note to change."),
                    "title": text("New title. English, ASCII only."),
                    "body": text("New body; replaces the old one. English, ASCII only."),
                    "append": text("Text added to the end of the body, instead of `body`. English, ASCII only."),
                    "status": one_of(Status::ALL, STATUS_RULES),
                    "tags": tags(),
                    "parent": note_id("New parent within the same project."),
                    "entity": entity_id("Entity this note is about, in the same project."),
                    "author": author(),
                    "run": run_label(),
                    "expected_revision": expected_revision(),
                    "confidence": confidence(),
                    "basis": basis(),
                }),
                &["id"],
            ),
        ),
        Tool::GetNote => (
            "Return one note in full, with its direct children and its links in both directions.",
            object(json!({ "id": note_id("Note to return.") }), &["id"]),
        ),
        Tool::Find => (
            "Full-text search over notes by exact words: identifiers, paths, error texts, names. \
             Returns matches with a snippet; `matched` is 'all' when every word matched, \
             'prefix' when every word matched the start of a word, 'any' when only some did.",
            object(
                json!({
                    "query": text("Words to look for, in English, ASCII only."),
                    "project": project("Project to search; '*' searches all. Defaults to the current project."),
                    "kind": one_of(NoteKind::ALL, "Only notes of this kind."),
                    "status": one_of(Status::ALL, "Only notes with this status."),
                    "under": note_id("Only this note and everything beneath it."),
                    "entity": entity_id("Only notes about this entity."),
                    "limit": limit(10),
                }),
                &["query"],
            ),
        ),
        Tool::Link => (
            "Connect two existing notes with a typed link, or remove that link with remove=true.",
            object(
                json!({
                    "src": note_id("Source note id."),
                    "dst": note_id("Target note id."),
                    "kind": one_of(EdgeKind::ALL, EDGE_RULES),
                    "remove": { "type": "boolean", "default": false },
                }),
                &["src", "dst", "kind"],
            ),
        ),
        Tool::Graph => (
            "Return the tree beneath a note or an entity in depth-first order, and every link \
             touching it. Nodes carry id, parent, depth and status, with kind and title for notes, \
             type and key for entities. Bodies are omitted.",
            object(
                json!({
                    "type": one_of(&[RecordType::Note, RecordType::Entity], "Which tree to read."),
                    "id": note_id("Root of the tree: a note, usually a goal, or an entity."),
                    "depth": { "type": "integer", "minimum": 1, "maximum": MAX_GRAPH_DEPTH, "default": DEFAULT_GRAPH_DEPTH },
                }),
                &["type", "id"],
            ),
        ),
        Tool::Resume => (
            "Without `task`: the state of a project - unfinished goals, steps in progress, open \
             questions, recent notes, entity counts, the last checkpoint, and `tasks` (each \
             unfinished goal with its last checkpoint time and the changes since). With `task`: \
             what is needed to continue it - its last checkpoint, progress, blockers, steps in \
             progress, failed attempts, changes after the checkpoint, next steps, decisions, and \
             under `checklist` every item still open. The task reply is cut to `budget_chars`; \
             `budget.omitted` counts what did not fit.",
            object(
                json!({
                    "project": project("Project to summarise; '*' covers all. Defaults to the current project."),
                    "task": task("Note id of the task to continue, usually a goal. Not together with `project`."),
                    "budget_chars": { "type": "integer", "minimum": tasks::MIN_BUDGET_CHARS,
                        "maximum": tasks::MAX_BUDGET_CHARS, "default": tasks::DEFAULT_BUDGET_CHARS,
                        "description": "Size of the task reply in characters. Only with `task`." },
                }),
                &[],
            ),
        ),
        Tool::Forget => (
            "Delete a note, entity or concept permanently with all revisions, links, checklist, \
             events and vector; only a journal entry that it existed remains. A note or entity \
             with children needs recursive=true. For what was once true, set a note to `dropped` \
             or archive the concept instead.",
            object(
                json!({
                    "type": one_of(
                        &[RecordType::Note, RecordType::Entity, RecordType::Concept],
                        "Kind of record to delete.",
                    ),
                    "id": record_id(),
                    "recursive": { "type": "boolean", "default": false,
                        "description": "Notes and entities only." },
                    "claim_id": text("Entities only: the claim_id from claim_entity, required while the entity is claimed."),
                    "author": author(),
                    "run": run_label(),
                }),
                &["type", "id"],
            ),
        ),
        Tool::Checkpoint => (
            "Save where the work stands, to be read back by `resume` after the context is lost. \
             With `task` it belongs to that task, so tasks in one project do not overwrite each \
             other. The latest 20 are kept per task and per project.",
            object(
                json!({
                    "summary": text("What is done, what is in progress, what comes next, and why. English, ASCII only."),
                    "project": project("Defaults to the task's project, else the current project."),
                    "task": task("Note id of the task this checkpoint is for, usually a goal."),
                    "author": author(),
                    "run": run_label(),
                }),
                &["summary"],
            ),
        ),
        Tool::Recall => (
            "Search notes, entities and concepts at once, most relevant first; `source` tells \
             them apart. Relevance comes from the words of the query and, when `semantic` is \
             true, from its meaning. Dropped or superseded notes rank lower and name their \
             replacement in `superseded_by`; age only breaks ties. Records saying the same thing \
             are shown once, the others listed under `duplicates` or `supersedes`; nothing is \
             deleted.",
            object(
                json!({
                    "query": text("What to look for, in English, ASCII only."),
                    "project": project("Project to search; '*' searches all. Defaults to the current project."),
                    "task": task("Note id of the task being worked on; its notes and entities rank a little higher."),
                    "only": one_of(
                        &[RecordType::Note, RecordType::Entity, RecordType::Concept],
                        "Search one store only, e.g. 'concept' for stored know-how.",
                    ),
                    "include_archived": { "type": "boolean", "default": false,
                        "description": "Also return archived concepts." },
                    "limit": limit(8),
                }),
                &["query"],
            ),
        ),
        Tool::UpsertEntity => (
            "Create or update a structured record of something being worked on, identified by \
             (project, type, key). Only the fields given are changed; `attrs` are merged, and a \
             null attribute removes it. `outcome` is 'created', 'updated' or 'unchanged'. Calling \
             it with just type and key returns the entity and its id without changing anything.",
            object(
                json!({
                    "type": token("Kind of thing, e.g. 'endpoint', 'file', 'task'."),
                    "key": text("Its natural name within the type, up to 256 characters."),
                    "project": project("Defaults to the current project."),
                    "status": token("Free-form state, e.g. 'discovered', 'verified'. Defaults to 'new'."),
                    "confidence": { "type": "number", "minimum": 0, "maximum": 1 },
                    "parent": entity_id("Entity this one belongs under, in the same project."),
                    "attrs": {
                        "type": "object",
                        "description": "Named scalar values (string, number, boolean); null removes one. Text in English, ASCII only.",
                        "additionalProperties": { "type": ["string", "number", "boolean", "null"] },
                    },
                    "author": author(),
                    "run": run_label(),
                    "task": task("Note id of the task this change belongs to, in the same project."),
                    "claim_id": claim_id(),
                }),
                &["type", "key"],
            ),
        ),
        Tool::GetEntity => (
            "Return one entity in full: its checks, children, links, the notes written about it \
             and its 20 latest events. `history` with type 'entity' pages through all of them.",
            object(json!({ "id": entity_id("Entity to return.") }), &["id"]),
        ),
        Tool::QueryEntities => (
            "Filter entities by field. Conditions in `where` are combined with AND. `total` \
             counts every match regardless of `limit`. With `group_by`, counts per value are \
             returned instead of entities.",
            object(
                json!({
                    "project": project("Project to query; '*' queries all. Defaults to the current project."),
                    "type": token("Only entities of this type."),
                    "where": {
                        "type": "array",
                        "maxItems": 16,
                        "items": object(
                            json!({
                                "field": text(FIELD_RULES),
                                "op": one_of(Op::ALL, "'ne' also matches entities where the field is absent. 'contains' is a case-sensitive substring test."),
                                "value": { "description": "A string, number or boolean; an array of them for 'in'; omitted for is_null and not_null." },
                            }),
                            &["field", "op"],
                        ),
                    },
                    "order_by": text(FIELD_RULES),
                    "descending": { "type": "boolean", "default": false },
                    "group_by": text(FIELD_RULES),
                    "limit": limit(20),
                }),
                &[],
            ),
        ),
        Tool::ClaimEntity => (
            "Reserve an entity. Until the claim expires or is released, only calls that present \
             the returned `claim_id` can change the entity; everyone can still read it. Fails with \
             a conflict when someone else holds it. Presenting the current claim_id renews it. \
             With release=true and the claim_id, the claim is given up before it expires.",
            object(
                json!({
                    "id": entity_id("Entity to reserve or to release."),
                    "ttl_seconds": { "type": "integer", "minimum": 1, "maximum": 86400, "default": 900,
                        "description": "Not with release." },
                    "author": author(),
                    "run": run_label(),
                    "claim_id": text("The current claim_id: to renew a claim, or required to release it."),
                    "release": { "type": "boolean", "default": false },
                }),
                &["id"],
            ),
        ),
        Tool::MarkCheck => (
            "Record the result of a named check on an entity. One result is kept per check name; \
             marking it again replaces it. Queries read it as `check.<name>`.",
            object(
                json!({
                    "id": entity_id("Entity that was checked."),
                    "name": token("Name of the check."),
                    "result": token("Outcome, e.g. 'pass', 'fail', 'skipped'."),
                    "detail": text("One line of explanation, up to 200 characters."),
                    "author": author(),
                    "run": run_label(),
                    "task": task("Note id of the task this check belongs to, in the same project."),
                    "claim_id": claim_id(),
                }),
                &["id", "name", "result"],
            ),
        ),
        Tool::LinkEntities => (
            "Connect two entities with a named link, read as 'src <kind> dst', or remove that \
             link with remove=true.",
            object(
                json!({
                    "src": entity_id("Source entity id."),
                    "dst": entity_id("Target entity id."),
                    "kind": token("Name of the relation, e.g. 'calls', 'depends_on'."),
                    "remove": { "type": "boolean", "default": false },
                }),
                &["src", "dst", "kind"],
            ),
        ),
        Tool::MemorizeConcept => (
            "Store reusable know-how for recall by meaning: the problem, what was found, how it \
             was checked, the outcome. Specifics belong in notes. `similar` lists existing \
             concepts that say nearly the same.",
            object(
                json!({
                    "title": text("One line, up to 200 characters. English, ASCII only."),
                    "content": text("The concept, written to be understood without its original context. English, ASCII only."),
                    "tags": tags(),
                    "project": project("Defaults to the current project."),
                    "sources": {
                        "type": "array", "items": { "type": "integer" }, "maxItems": 16,
                        "description": "Ids of the notes this concept was distilled from.",
                    },
                    "author": author(),
                    "run": run_label(),
                    "task": task("Note id of the task this concept came out of, in the same project."),
                    "confidence": confidence(),
                    "basis": basis(),
                }),
                &["title", "content"],
            ),
        ),
        Tool::UpdateConcept => (
            "Change a concept's title, content or tags, or archive it. Omitted fields stay. Each \
             change is a new revision. An archived concept keeps its history but is left out of \
             searches; `archived: false` brings it back.",
            object(
                json!({
                    "id": text("Concept id (UUID)."),
                    "title": text("New title. English, ASCII only."),
                    "content": text("New content; replaces the old one. English, ASCII only."),
                    "tags": tags(),
                    "archived": { "type": "boolean", "description": "Archive or restore the concept." },
                    "author": author(),
                    "run": run_label(),
                    "expected_revision": expected_revision(),
                    "confidence": confidence(),
                    "basis": basis(),
                }),
                &["id"],
            ),
        ),
        Tool::History => (
            "List changes, newest first: record, `op`, changed `fields`, `author`, `run`, `task`, \
             time. Give `type` and `id` for one record, `task` for one task, or neither for the \
             project. The journal holds field names, not text; `get_revision` returns text. \
             `origin` is 'recorded', 'legacy_baseline' (the state when history began; what came \
             before is unknown) or 'backfilled' (copied from older logs). Pass `next_cursor` as \
             `cursor` for the next page.",
            object(
                json!({
                    "type": one_of(RecordType::ALL, "Only records of this kind."),
                    "id": record_id(),
                    "project": project("Project to list; '*' lists all. Defaults to the current project. Ignored with `id`."),
                    "task": task("Only changes attributed to this task or made to the notes beneath it."),
                    "cursor": { "type": "integer", "minimum": 1, "description": "`next_cursor` of the previous page." },
                    "limit": limit(20),
                }),
                &[],
            ),
        ),
        Tool::GetRevision => (
            "Return the state a note or concept had at a given revision, or its current state \
             when `revision` is omitted. The 50 latest revisions of a record are kept.",
            object(
                json!({
                    "type": one_of(&[RecordType::Note, RecordType::Concept], "Kind of record."),
                    "id": record_id(),
                    "revision": { "type": "integer", "minimum": 1 },
                }),
                &["type", "id"],
            ),
        ),
        Tool::RedactHistory => (
            "Permanently delete every earlier revision of a note or concept, keeping its current \
             state. Use it after removing something that should never have been stored, such as \
             a secret, from the record: editing alone leaves the old text in the revisions.",
            object(
                json!({
                    "type": one_of(&[RecordType::Note, RecordType::Concept], "Kind of record."),
                    "id": record_id(),
                    "author": author(),
                    "run": run_label(),
                }),
                &["type", "id"],
            ),
        ),
        Tool::Checklist => (
            "Checklist on a note, usually a step: what has to be done or checked. An item is done \
             or not done, nothing in between. Tick each item the moment it is finished. One call \
             can add, tick, untick and remove; with only `note` it reads. Returns the whole list \
             with `done` and `total`.",
            object(
                json!({
                    "note": note_id("Note the checklist belongs to."),
                    "add": { "type": "array", "maxItems": 32,
                        "items": { "type": "string" },
                        "description": "New items, one line each, up to 200 characters. English, ASCII only." },
                    "done": item_ids("Ids of items that are now done."),
                    "undone": item_ids("Ids of items ticked by mistake."),
                    "remove": item_ids("Ids of items that no longer apply."),
                    "author": author(),
                    "run": run_label(),
                }),
                &["note"],
            ),
        ),
        Tool::Summarize => (
            "Close a goal or step with its one summary, written while you still see everything. \
             `work`: all that was done, in brief, failures included. `result`: the outcome in \
             detail, usable without the task's notes. Calling again replaces it; the old text \
             stays as a revision. `parts_without_summary` names finished sub-tasks never \
             summarised.",
            object(
                json!({
                    "task": task("The goal or step being summarised."),
                    "result": text("The outcome in detail: findings, values, what holds and what does not. English, ASCII only."),
                    "work": text("Everything that was done, in brief, one line per action, failed attempts included. English, ASCII only."),
                    "author": author(),
                    "run": run_label(),
                }),
                &["task", "result", "work"],
            ),
        ),
        Tool::TaskSummary => (
            "Read a task from summaries alone: its own, then those of the goals and steps beneath \
             it in tree order, plus checklist items still open. No other notes are returned. Use \
             it to answer 'what was done and found here' and before summarising a parent task.",
            object(
                json!({
                    "task": task("The goal or step to read."),
                    "budget_chars": budget_chars(),
                }),
                &["task"],
            ),
        ),
        Tool::Confirm => (
            "Record that a note or concept was checked again now and still holds, without \
             changing its text. Sets `verified_at`, and `confidence` and `basis` when given. If \
             it proved wrong, correct or supersede it instead.",
            object(
                json!({
                    "type": one_of(&[RecordType::Note, RecordType::Concept], "Kind of record."),
                    "id": record_id(),
                    "confidence": confidence(),
                    "basis": basis(),
                    "author": author(),
                    "run": run_label(),
                }),
                &["type", "id"],
            ),
        ),
        Tool::Review => (
            "What in a project needs attention: stalled goals and steps, open questions, finished \
             tasks without a summary, open checklist items, loose notes, claims, low-confidence \
             records, concepts not checked lately, records awaiting indexing. Read-only.",
            object(
                json!({
                    "project": project("Project to review; '*' covers all. Defaults to the current project."),
                    "stale_days": { "type": "integer", "minimum": 1, "maximum": 365, "default": 14 },
                }),
                &[],
            ),
        ),
    }
}
