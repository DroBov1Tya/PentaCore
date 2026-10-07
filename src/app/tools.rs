use anyhow::Context;
use serde::Deserialize;
use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::{Value, json};
use std::fmt;

use super::memory::Brain;
use super::memory::concepts::{ConceptId, ConceptQuery};
use super::memory::entities::Op;
use super::memory::model::{
    Edge, EdgeKind, FindQuery, Limit, MAX_GRAPH_DEPTH, MemoryError, NoteId, NoteKind, Scope,
    SearchText, Status, invalid, string_enum,
};

const DEFAULT_GRAPH_DEPTH: u8 = 8;
const DEFAULT_RECALL_LIMIT: usize = 8;
const RECALL_PREVIEW_CHARS: usize = 300;

string_enum!(Tool {
    Note => "note",
    UpdateNote => "update_note",
    GetNote => "get_note",
    Find => "find",
    Link => "link",
    Unlink => "unlink",
    GoalGraph => "goal_graph",
    Resume => "resume",
    ForgetNote => "forget_note",
    Checkpoint => "checkpoint",
    Recall => "recall",
    UpsertEntity => "upsert_entity",
    GetEntity => "get_entity",
    QueryEntities => "query_entities",
    ClaimEntity => "claim_entity",
    ReleaseEntity => "release_entity",
    MarkCheck => "mark_check",
    LinkEntities => "link_entities",
    EntityGraph => "entity_graph",
    ForgetEntity => "forget_entity",
    MemorizeConcept => "memorize_concept",
    SearchConcepts => "search_concepts",
    UpdateConcept => "update_concept",
    ForgetConcept => "forget_concept",
});

impl Tool {
    pub fn changes_memory(self) -> bool {
        !matches!(
            self,
            Self::GetNote
                | Self::Find
                | Self::GoalGraph
                | Self::Resume
                | Self::Recall
                | Self::GetEntity
                | Self::QueryEntities
                | Self::EntityGraph
                | Self::SearchConcepts
        )
    }

    pub fn deletes(self) -> bool {
        matches!(
            self,
            Self::ForgetNote | Self::ForgetEntity | Self::ForgetConcept
        )
    }
}

// Sent to the agent once, on connect.
pub const INSTRUCTIONS: &str = "\
pentacore is a persistent working memory shared across sessions and agents. It stores what it is \
given and returns what is asked for; it does not plan, rank or decide.

Three stores:
- Notes hold narrative specifics and are found by exact words (`find`): goals, steps, attempts and \
their outcomes, facts (paths, commands, error texts, values), decisions with their reasons, open \
questions. Notes form a tree through `parent` and a graph through typed links.
- Entities hold operational state as structured records (`upsert_entity`, `query_entities`): the \
things being worked on, each with a type, a key, a status, a confidence, attributes and named \
checks. They are filtered, counted and grouped by field, with no text to parse. A note can point \
at the entity it is about.
- Concepts hold generalised, reusable knowledge and are found by meaning (`search_concepts`).

`recall` searches notes and concepts at once. Working notes that would otherwise go into scratch \
files belong here, so they survive the session. `resume` returns the current state of a project \
and the last `checkpoint`; `goal_graph` and `entity_graph` return trees.

Several agents may share one project. `claim_entity` reserves an entity for one of them; `author` \
is a label on what was written and grants nothing.";

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct IdArgs {
    id: NoteId,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct EdgeArgs {
    src: NoteId,
    dst: NoteId,
    kind: EdgeKind,
}

impl From<EdgeArgs> for Edge {
    fn from(args: EdgeArgs) -> Self {
        Self {
            src: args.src,
            dst: args.dst,
            kind: args.kind,
        }
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct GraphArgs {
    id: NoteId,
    depth: Option<u8>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ScopeArgs {
    project: Option<Scope>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ForgetNoteArgs {
    id: NoteId,
    #[serde(default)]
    recursive: bool,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RecallArgs {
    query: SearchText,
    project: Option<Scope>,
    limit: Option<Limit>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ConceptIdArgs {
    id: ConceptId,
}

// Logs writes by tool name only. Internal causes are logged, never returned.
pub async fn call(brain: &Brain, tool: Tool, args: Value) -> Result<Value, MemoryError> {
    let result = run(brain, tool, args).await;
    match &result {
        Ok(_) if tool.changes_memory() => tracing::info!(%tool, "memory changed"),
        Err(MemoryError::Internal(cause)) => tracing::error!(%tool, "tool failed: {cause:#}"),
        _ => {}
    }
    result
}

async fn run(brain: &Brain, tool: Tool, args: Value) -> Result<Value, MemoryError> {
    let project = || brain.default_project.clone();
    match tool {
        Tool::Note => to_json(brain.notes.create(parse(args)?, project()).await?),
        Tool::UpdateNote => to_json(brain.notes.update(parse(args)?).await?),
        Tool::GetNote => {
            let IdArgs { id } = parse(args)?;
            to_json(brain.notes.get(id).await?)
        }
        Tool::Find => to_json(
            brain
                .notes
                .find(parse(args)?, brain.default_scope())
                .await?,
        ),
        Tool::Link => {
            let edge: EdgeArgs = parse(args)?;
            to_json(brain.notes.link(edge.into()).await?)
        }
        Tool::Unlink => {
            let edge: EdgeArgs = parse(args)?;
            Ok(json!({ "removed": brain.notes.unlink(edge.into()).await? }))
        }
        Tool::GoalGraph => {
            let GraphArgs { id, depth } = parse(args)?;
            to_json(brain.notes.graph(id, graph_depth(depth)?).await?)
        }
        Tool::Resume => {
            let ScopeArgs { project } = parse(args)?;
            let scope = project.unwrap_or_else(|| brain.default_scope());
            let mut state = to_json(brain.notes.snapshot(scope.clone()).await?)?;
            state["entities"] = to_json(brain.entities.summary(scope).await?)?;
            Ok(state)
        }
        Tool::Checkpoint => to_json(brain.notes.checkpoint(parse(args)?, project()).await?),
        Tool::Recall => recall(brain, parse(args)?).await,
        Tool::UpsertEntity => to_json(brain.entities.upsert(parse(args)?, project()).await?),
        Tool::GetEntity => {
            let IdArgs { id } = parse(args)?;
            to_json(brain.entities.get(id).await?)
        }
        Tool::QueryEntities => to_json(
            brain
                .entities
                .query(parse(args)?, brain.default_scope())
                .await?,
        ),
        Tool::ClaimEntity => to_json(brain.entities.claim(parse(args)?).await?),
        Tool::ReleaseEntity => {
            Ok(json!({ "released": brain.entities.release(parse(args)?).await? }))
        }
        Tool::MarkCheck => to_json(brain.entities.mark_check(parse(args)?).await?),
        Tool::LinkEntities => Ok(json!({ "linked": brain.entities.link(parse(args)?).await? })),
        Tool::EntityGraph => {
            let GraphArgs { id, depth } = parse(args)?;
            to_json(brain.entities.graph(id, graph_depth(depth)?).await?)
        }
        Tool::ForgetEntity => Ok(json!({ "deleted": brain.entities.forget(parse(args)?).await? })),
        Tool::ForgetNote => {
            let ForgetNoteArgs { id, recursive } = parse(args)?;
            Ok(json!({ "deleted": brain.notes.forget(id, recursive).await? }))
        }
        Tool::MemorizeConcept => to_json(brain.concepts.memorize(parse(args)?, project()).await?),
        Tool::SearchConcepts => to_json(
            brain
                .concepts
                .search(parse(args)?, brain.default_scope())
                .await?,
        ),
        Tool::UpdateConcept => to_json(brain.concepts.update(parse(args)?).await?),
        Tool::ForgetConcept => {
            let ConceptIdArgs { id } = parse(args)?;
            brain.concepts.forget(id).await?;
            Ok(json!({ "deleted": 1 }))
        }
    }
}

fn graph_depth(depth: Option<u8>) -> Result<u8, MemoryError> {
    let depth = depth.unwrap_or(DEFAULT_GRAPH_DEPTH);
    if !(1..=MAX_GRAPH_DEPTH).contains(&depth) {
        return Err(invalid(format!(
            "depth must be between 1 and {MAX_GRAPH_DEPTH}"
        )));
    }
    Ok(depth)
}

// The two scores are not comparable, so results alternate between stores.
async fn recall(brain: &Brain, args: RecallArgs) -> Result<Value, MemoryError> {
    let limit = Limit::or(args.limit, DEFAULT_RECALL_LIMIT);
    let scope = args.project.unwrap_or_else(|| brain.default_scope());
    let note_query = FindQuery {
        query: args.query.clone(),
        project: Some(scope.clone()),
        kind: None,
        status: None,
        under: None,
        entity: None,
        limit: args.limit,
    };
    let concept_query = ConceptQuery {
        query: args.query,
        project: Some(scope.clone()),
        limit: args.limit,
    };
    let (notes, concepts) = tokio::join!(
        brain.notes.find(note_query, scope.clone()),
        brain.concepts.search(concept_query, scope),
    );

    let notes = notes?;
    let notes_matched = notes.matched;
    // Without the embedding model, recall answers from notes only.
    let (concepts, concepts_available) = match concepts {
        Ok(hits) => (hits, true),
        Err(MemoryError::Internal(cause)) => {
            tracing::warn!("recall: concept search unavailable: {cause:#}");
            (Vec::new(), false)
        }
        Err(other) => return Err(other),
    };

    let mut note_results = Vec::new();
    for hit in notes.hits {
        let mut result = to_json(hit)?;
        result["source"] = json!("note");
        note_results.push(result);
    }
    let concept_results = concepts.into_iter().map(|hit| {
        json!({
            "source": "concept",
            "id": hit.concept.id,
            "project": hit.concept.project,
            "title": hit.concept.title,
            "preview": hit.concept.content.chars().take(RECALL_PREVIEW_CHARS).collect::<String>(),
            "score": hit.score,
        })
    });

    let mut results = Vec::new();
    let (mut note_results, mut concept_results) = (note_results.into_iter(), concept_results);
    while results.len() < limit {
        let (note, concept) = (note_results.next(), concept_results.next());
        if note.is_none() && concept.is_none() {
            break;
        }
        results.extend(note.into_iter().chain(concept));
    }
    results.truncate(limit);

    Ok(json!({
        "results": results,
        "notes_matched": notes_matched,
        "concepts_available": concepts_available,
    }))
}

fn parse<T: DeserializeOwned>(args: Value) -> Result<T, MemoryError> {
    let args = if args.is_null() { json!({}) } else { args };
    serde_json::from_value(args).map_err(|error| invalid(error.to_string()))
}

fn to_json(value: impl Serialize) -> Result<Value, MemoryError> {
    Ok(serde_json::to_value(value).context("failed to encode a tool result")?)
}

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
            "description": "Short labels without spaces." })
}

fn project(description: &str) -> Value {
    text(description)
}

fn entity_id(description: &str) -> Value {
    json!({ "type": "integer", "description": description })
}

fn author() -> Value {
    text("Label for who wrote this, e.g. 'claude/reviewer'. Informational only.")
}

fn claim_id() -> Value {
    text("The claim_id from claim_entity; required while the entity is claimed.")
}

fn token(description: &str) -> Value {
    json!({ "type": "string", "pattern": "^[a-z0-9][a-z0-9_.-]{0,47}$", "description": description })
}

const FIELD_RULES: &str = "A column (id, type, key, status, confidence, parent, author, \
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
                    "title": text("One line, up to 200 characters."),
                    "body": text("The specifics: exact paths, commands, error texts, values, reasoning."),
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
                }),
                &["kind", "title"],
            ),
        ),
        Tool::UpdateNote => (
            "Change a note's title, body, status, tags, parent or entity. Omitted fields stay as \
             they are.",
            object(
                json!({
                    "id": note_id("Note to change."),
                    "title": text("New title."),
                    "body": text("New body; replaces the old one."),
                    "append": text("Text added to the end of the body, instead of `body`."),
                    "status": one_of(Status::ALL, STATUS_RULES),
                    "tags": tags(),
                    "parent": note_id("New parent within the same project."),
                    "entity": entity_id("Entity this note is about, in the same project."),
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
                    "query": text("Words to look for."),
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
            "Connect two existing notes with a typed link.",
            object(
                json!({
                    "src": note_id("Source note id."),
                    "dst": note_id("Target note id."),
                    "kind": one_of(EdgeKind::ALL, EDGE_RULES),
                }),
                &["src", "dst", "kind"],
            ),
        ),
        Tool::Unlink => (
            "Remove a typed link between two notes.",
            object(
                json!({
                    "src": note_id("Source note id."),
                    "dst": note_id("Target note id."),
                    "kind": one_of(EdgeKind::ALL, "Kind of the link to remove."),
                }),
                &["src", "dst", "kind"],
            ),
        ),
        Tool::GoalGraph => (
            "Return the tree beneath a note in depth-first order (id, parent, depth, kind, status, \
             title) and every link touching it. Bodies are omitted; use get_note for them.",
            object(
                json!({
                    "id": note_id("Root of the tree, usually a goal."),
                    "depth": { "type": "integer", "minimum": 1, "maximum": MAX_GRAPH_DEPTH, "default": DEFAULT_GRAPH_DEPTH },
                }),
                &["id"],
            ),
        ),
        Tool::Resume => (
            "Return the current state of a project: unfinished goals, steps in progress, open \
             questions, the most recently touched notes, entity counts by type and status, and \
             the last checkpoint.",
            object(
                json!({ "project": project("Project to summarise; '*' covers all. Defaults to the current project.") }),
                &[],
            ),
        ),
        Tool::ForgetNote => (
            "Delete a note and its links permanently. A note with notes beneath it is deleted only \
             with recursive=true, which removes the whole subtree.",
            object(
                json!({
                    "id": note_id("Note to delete."),
                    "recursive": { "type": "boolean", "default": false },
                }),
                &["id"],
            ),
        ),
        Tool::Checkpoint => (
            "Save a summary of where the work stands, to be read back by `resume` after the \
             context is lost or compacted. The latest checkpoint of a project is the one returned.",
            object(
                json!({
                    "summary": text("What is done, what is in progress, what comes next, and why."),
                    "project": project("Defaults to the current project."),
                    "author": author(),
                }),
                &["summary"],
            ),
        ),
        Tool::Recall => (
            "Search notes by words and concepts by meaning in one call. Results alternate between \
             the two sources, best of each first; `source` tells them apart.",
            object(
                json!({
                    "query": text("What to look for."),
                    "project": project("Project to search; '*' searches all. Defaults to the current project."),
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
                        "description": "Named scalar values (string, number, boolean); null removes one.",
                        "additionalProperties": { "type": ["string", "number", "boolean", "null"] },
                    },
                    "author": author(),
                    "claim_id": claim_id(),
                }),
                &["type", "key"],
            ),
        ),
        Tool::GetEntity => (
            "Return one entity in full: its checks, children, links, the notes written about it \
             and its recent history.",
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
             a conflict when someone else holds it. Presenting the current claim_id renews it.",
            object(
                json!({
                    "id": entity_id("Entity to reserve."),
                    "ttl_seconds": { "type": "integer", "minimum": 1, "maximum": 86400, "default": 900 },
                    "author": author(),
                    "claim_id": text("The current claim_id, to renew a claim already held."),
                }),
                &["id"],
            ),
        ),
        Tool::ReleaseEntity => (
            "Give up a claim before it expires.",
            object(
                json!({
                    "id": entity_id("Claimed entity."),
                    "claim_id": text("The claim_id from claim_entity."),
                    "author": author(),
                }),
                &["id", "claim_id"],
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
        Tool::EntityGraph => (
            "Return the tree beneath an entity in depth-first order (id, parent, depth, type, key, \
             status) and every link touching it.",
            object(
                json!({
                    "id": entity_id("Root of the tree."),
                    "depth": { "type": "integer", "minimum": 1, "maximum": MAX_GRAPH_DEPTH, "default": DEFAULT_GRAPH_DEPTH },
                }),
                &["id"],
            ),
        ),
        Tool::ForgetEntity => (
            "Delete an entity with its checks, links and history permanently. An entity with \
             others beneath it is deleted only with recursive=true. Notes about it are kept.",
            object(
                json!({
                    "id": entity_id("Entity to delete."),
                    "recursive": { "type": "boolean", "default": false },
                    "claim_id": claim_id(),
                }),
                &["id"],
            ),
        ),
        Tool::MemorizeConcept => (
            "Store generalised, reusable knowledge (a principle, a pattern, a lesson) for recall by \
             meaning. Concrete specifics belong in notes instead. The reply lists `similar` \
             existing concepts that say nearly the same.",
            object(
                json!({
                    "title": text("One line, up to 200 characters."),
                    "content": text("The concept, written to be understood without its original context."),
                    "tags": tags(),
                    "project": project("Defaults to the current project."),
                    "sources": {
                        "type": "array", "items": { "type": "integer" }, "maxItems": 16,
                        "description": "Ids of the notes this concept was distilled from.",
                    },
                }),
                &["title", "content"],
            ),
        ),
        Tool::SearchConcepts => (
            "Semantic search over concepts: finds related ideas even when the wording differs. \
             `score` is between 0 and 1, higher is closer.",
            object(
                json!({
                    "query": text("What to recall, in natural language."),
                    "project": project("Project to search; '*' searches all. Defaults to the current project."),
                    "limit": limit(5),
                }),
                &["query"],
            ),
        ),
        Tool::UpdateConcept => (
            "Change a concept's title, content or tags. Omitted fields stay as they are.",
            object(
                json!({
                    "id": text("Concept id (UUID)."),
                    "title": text("New title."),
                    "content": text("New content; replaces the old one."),
                    "tags": tags(),
                }),
                &["id"],
            ),
        ),
        Tool::ForgetConcept => (
            "Delete a concept permanently.",
            object(json!({ "id": text("Concept id (UUID).") }), &["id"]),
        ),
    }
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use crate::app::memory::concepts::ConceptStore;
    use crate::app::memory::db::Db;
    use crate::app::memory::entities::{Column, EntityStore};
    use crate::app::memory::model::ProjectName;
    use crate::app::memory::notes::NoteStore;

    pub fn brain() -> (Brain, tempfile::TempDir) {
        let dir = tempfile::tempdir().unwrap();
        let db = Db::in_memory();
        let brain = Brain {
            notes: NoteStore::new(db.clone()),
            entities: EntityStore::new(db),
            concepts: ConceptStore::new(
                dir.path().join("concepts.lancedb"),
                dir.path().join("models"),
            ),
            default_project: ProjectName::try_from("demo".to_string()).unwrap(),
        };
        (brain, dir)
    }

    #[test]
    fn every_tool_has_a_closed_schema_whose_required_fields_exist() {
        let definitions = definitions();
        let tools = definitions["tools"].as_array().unwrap();
        assert_eq!(tools.len(), Tool::ALL.len());
        for tool in tools {
            let schema = &tool["inputSchema"];
            assert_eq!(schema["additionalProperties"], false, "{}", tool["name"]);
            for field in schema["required"].as_array().unwrap() {
                assert!(
                    schema["properties"].get(field.as_str().unwrap()).is_some(),
                    "{}",
                    tool["name"]
                );
            }
        }
    }

    #[tokio::test]
    async fn a_goal_tree_can_be_built_and_read_back() {
        let (brain, _dir) = brain();
        let goal = call(
            &brain,
            Tool::Note,
            json!({"kind": "goal", "title": "Ship v1"}),
        )
        .await
        .unwrap();
        let step = call(
            &brain,
            Tool::Note,
            json!({"kind": "step", "title": "Write schema", "parent": goal["id"]}),
        )
        .await
        .unwrap();
        call(
            &brain,
            Tool::UpdateNote,
            json!({"id": step["id"], "status": "active"}),
        )
        .await
        .unwrap();

        let graph = call(&brain, Tool::GoalGraph, json!({"id": goal["id"]}))
            .await
            .unwrap();
        assert_eq!(graph["nodes"].as_array().unwrap().len(), 2);
        assert_eq!(graph["nodes"][1]["status"], "active");

        let resume = call(&brain, Tool::Resume, Value::Null).await.unwrap();
        assert_eq!(resume["goals"][0]["id"], goal["id"]);
        assert_eq!(resume["active_steps"][0]["id"], step["id"]);

        let found = call(&brain, Tool::Find, json!({"query": "schema"}))
            .await
            .unwrap();
        assert_eq!(found["hits"][0]["id"], step["id"]);
        assert!(
            found["hits"][0]["snippet"]
                .as_str()
                .unwrap()
                .contains("[schema]")
        );
    }

    #[test]
    fn the_field_rules_name_every_queryable_column() {
        for column in Column::ALL {
            assert!(FIELD_RULES.contains(column.as_str()), "{column}");
        }
    }

    #[tokio::test]
    async fn operational_state_is_queryable_through_the_tools() {
        let (brain, _dir) = brain();
        for (key, status) in [
            ("/users", "discovered"),
            ("/orders", "discovered"),
            ("/health", "ignored"),
        ] {
            call(
                &brain,
                Tool::UpsertEntity,
                json!({"type": "endpoint", "key": key, "status": status}),
            )
            .await
            .unwrap();
        }
        let users = call(
            &brain,
            Tool::UpsertEntity,
            json!({"type": "endpoint", "key": "/users"}),
        )
        .await
        .unwrap();
        assert_eq!(users["outcome"], "unchanged");
        call(
            &brain,
            Tool::MarkCheck,
            json!({"id": users["id"], "name": "reviewed", "result": "pass"}),
        )
        .await
        .unwrap();
        call(
            &brain,
            Tool::Note,
            json!({"kind": "fact", "title": "Paginates by cursor", "entity": users["id"]}),
        )
        .await
        .unwrap();

        let pending = call(
            &brain,
            Tool::QueryEntities,
            json!({"type": "endpoint", "where": [
                {"field": "status", "op": "eq", "value": "discovered"},
                {"field": "check.reviewed", "op": "is_null"},
            ]}),
        )
        .await
        .unwrap();
        assert_eq!(pending["total"], 1);
        assert_eq!(pending["entities"][0]["key"], "/orders");

        let grant = call(
            &brain,
            Tool::ClaimEntity,
            json!({"id": users["id"], "author": "a"}),
        )
        .await
        .unwrap();
        let blocked = call(
            &brain,
            Tool::UpsertEntity,
            json!({"type": "endpoint", "key": "/users", "status": "done"}),
        )
        .await;
        assert!(matches!(blocked, Err(MemoryError::Conflict(_))));
        let released = call(
            &brain,
            Tool::ReleaseEntity,
            json!({"id": users["id"], "claim_id": grant["claim_id"]}),
        )
        .await
        .unwrap();
        assert_eq!(released["released"], true);

        let detail = call(&brain, Tool::GetEntity, json!({"id": users["id"]}))
            .await
            .unwrap();
        assert_eq!(detail["notes"][0]["title"], "Paginates by cursor");
        assert_eq!(detail["entity"]["checks"]["reviewed"], "pass");

        call(
            &brain,
            Tool::Checkpoint,
            json!({"summary": "Two endpoints left to review."}),
        )
        .await
        .unwrap();
        let resume = call(&brain, Tool::Resume, Value::Null).await.unwrap();
        assert_eq!(
            resume["checkpoint"]["summary"],
            "Two endpoints left to review."
        );
        assert_eq!(
            resume["entities"],
            json!([
                {"type": "endpoint", "status": "discovered", "count": 2},
                {"type": "endpoint", "status": "ignored", "count": 1},
            ])
        );
    }

    #[tokio::test]
    async fn malformed_arguments_are_invalid_input() {
        let (brain, _dir) = brain();
        for (tool, args) in [
            (Tool::Note, json!({"kind": "goal"})),
            (
                Tool::Note,
                json!({"kind": "goal", "title": "t", "extra": 1}),
            ),
            (Tool::Note, json!("not an object")),
            (Tool::GetNote, json!({"id": "1"})),
            (Tool::GetNote, Value::Null),
            (Tool::Find, json!({"query": "x", "limit": 0})),
            (Tool::Find, json!({"query": "x", "limit": 101})),
            (Tool::Find, json!({"query": "x", "project": "a b"})),
            (Tool::GoalGraph, json!({"id": 1, "depth": 0})),
            (Tool::GoalGraph, json!({"id": 1, "depth": 33})),
            (Tool::Link, json!({"src": 1, "dst": 2, "kind": "owns"})),
            (Tool::UpsertEntity, json!({"type": "endpoint"})),
            (
                Tool::QueryEntities,
                json!({"where": [{"field": "status); --", "op": "eq", "value": 1}]}),
            ),
            (Tool::QueryEntities, json!({"sql": "SELECT 1"})),
            (Tool::EntityGraph, json!({"id": 1, "depth": 0})),
            (Tool::ClaimEntity, json!({"id": 1, "ttl_seconds": -5})),
            (Tool::Checkpoint, json!({"summary": ""})),
            (Tool::Recall, json!({"query": ""})),
            (Tool::ForgetConcept, json!({"id": "not-a-uuid"})),
            (
                Tool::MemorizeConcept,
                json!({"title": "t", "content": "   "}),
            ),
        ] {
            let result = call(&brain, tool, args.clone()).await;
            assert!(
                matches!(result, Err(MemoryError::Invalid(_))),
                "{tool} {args}"
            );
        }
    }
}
