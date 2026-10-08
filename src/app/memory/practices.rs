// Checklists (done or not), one summary per task, confidence with its basis,
// and a review of what needs attention.

use rusqlite::{Connection, OptionalExtension, TransactionBehavior, params};
use serde::Deserialize;
use serde_json::{Map, Value, json};

use super::entities::Confidence;
use super::journal::{self, Entry, Op, RecordId, RecordType};
use super::model::{
    AtMost, Attribution, Author, Body, MemoryError, NewNote, NoteId, NoteKind, NotePatch,
    ProjectName, RunId, Scope, Title, invalid, now, seconds_from_now, validated_string,
};
use super::notes;
use super::tasks::{DEFAULT_BUDGET_CHARS, MAX_BUDGET_CHARS, MIN_BUDGET_CHARS, SUBTREE, truncated};

const MAX_ITEMS_PER_NOTE: i64 = 100;
const MAX_BASIS_CHARS: usize = 500;
const MAX_SUMMARY_TITLE_CHARS: usize = 180;
const LISTED: i64 = 20;
const SECTION_ROWS: i64 = 200;
const DEFAULT_STALE_DAYS: u32 = 14;
const MAX_STALE_DAYS: u32 = 365;

type Result<T> = std::result::Result<T, MemoryError>;

fn internal(error: serde_json::Error) -> MemoryError {
    MemoryError::Internal(error.into())
}

// ---------------------------------------------------------------- checklists

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ChecklistChange {
    pub note: NoteId,
    #[serde(default)]
    pub add: AtMost<Title, 32>,
    #[serde(default)]
    pub done: AtMost<i64, 64>,
    #[serde(default)]
    pub undone: AtMost<i64, 64>,
    #[serde(default)]
    pub remove: AtMost<i64, 64>,
    pub author: Option<Author>,
    pub run: Option<RunId>,
}

fn item_not_found(item: i64, note: NoteId) -> MemoryError {
    MemoryError::NotFound(format!("checklist item {item} of note {note}"))
}

// The checklist of a note: every item, and how many are done.
pub fn checklist_of(conn: &Connection, note: NoteId) -> Result<Value> {
    let items: Vec<Value> = conn
        .prepare(
            "SELECT id, text, done, done_by, done_at FROM checklist_items
             WHERE note_id = ?1 ORDER BY id LIMIT ?2",
        )?
        .query_map(params![note, MAX_ITEMS_PER_NOTE], |row| {
            let done: bool = row.get(2)?;
            let mut item = json!({
                "id": row.get::<_, i64>(0)?,
                "text": row.get::<_, String>(1)?,
                "done": done,
            });
            if done {
                item["done_by"] = json!(row.get::<_, Option<String>>(3)?);
                item["done_at"] = json!(row.get::<_, Option<String>>(4)?);
            }
            Ok(item)
        })?
        .collect::<rusqlite::Result<_>>()?;
    let done = items.iter().filter(|item| item["done"] == true).count();
    Ok(json!({ "note": note, "done": done, "total": items.len(), "items": items }))
}

// Applies the change in one transaction and returns the list.
pub fn change_checklist(conn: &mut Connection, change: ChecklistChange) -> Result<Value> {
    let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let note = notes::load(&tx, change.note)?;
    let who = Attribution::new(change.author, change.run, None);
    let at = now();
    let mut changed = false;

    for item in change.done.as_slice() {
        let ticked = tx.execute(
            "UPDATE checklist_items SET done = 1, done_by = ?3, done_at = ?4
             WHERE id = ?1 AND note_id = ?2",
            params![item, note.id, who.author, at],
        )?;
        if ticked == 0 {
            return Err(item_not_found(*item, note.id));
        }
        changed = true;
    }
    for item in change.undone.as_slice() {
        let unticked = tx.execute(
            "UPDATE checklist_items SET done = 0, done_by = NULL, done_at = NULL
             WHERE id = ?1 AND note_id = ?2",
            params![item, note.id],
        )?;
        if unticked == 0 {
            return Err(item_not_found(*item, note.id));
        }
        changed = true;
    }
    for item in change.remove.as_slice() {
        let removed = tx.execute(
            "DELETE FROM checklist_items WHERE id = ?1 AND note_id = ?2",
            params![item, note.id],
        )?;
        if removed == 0 {
            return Err(item_not_found(*item, note.id));
        }
        changed = true;
    }
    if !change.add.as_slice().is_empty() {
        let existing: i64 = tx.query_row(
            "SELECT count(*) FROM checklist_items WHERE note_id = ?1",
            [note.id],
            |row| row.get(0),
        )?;
        if existing + change.add.as_slice().len() as i64 > MAX_ITEMS_PER_NOTE {
            return Err(invalid(format!(
                "a note holds at most {MAX_ITEMS_PER_NOTE} checklist items; split the work into steps"
            )));
        }
        for text in change.add.as_slice() {
            tx.execute(
                "INSERT INTO checklist_items (note_id, text, created_at) VALUES (?1, ?2, ?3)",
                params![note.id, text.as_str(), at],
            )?;
        }
        changed = true;
    }

    if changed {
        journal::record(
            &tx,
            Entry::new(&note.project, RecordType::Note, note.id, Op::Updated, &who)
                .fields(["checklist"]),
        )?;
    }
    let list = checklist_of(&tx, note.id)?;
    tx.commit()?;
    Ok(list)
}

// What is still to do in a task and everything beneath it.
pub fn open_items(conn: &Connection, task: NoteId) -> Result<Value> {
    let (done, total): (i64, i64) = conn.query_row(
        &format!(
            "SELECT coalesce(sum(done), 0), count(*) FROM checklist_items
             WHERE note_id IN ({SUBTREE})"
        ),
        [task],
        |row| Ok((row.get(0)?, row.get(1)?)),
    )?;
    let open: Vec<Value> = conn
        .prepare(&format!(
            "SELECT id, note_id, text FROM checklist_items
             WHERE done = 0 AND note_id IN ({SUBTREE}) ORDER BY note_id, id LIMIT ?2"
        ))?
        .query_map(params![task, MAX_ITEMS_PER_NOTE], |row| {
            Ok(json!({
                "id": row.get::<_, i64>(0)?,
                "note": row.get::<_, i64>(1)?,
                "text": row.get::<_, String>(2)?,
            }))
        })?
        .collect::<rusqlite::Result<_>>()?;
    Ok(json!({ "done": done, "total": total, "open": open }))
}

// ----------------------------------------------------------------- summaries

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Summarize {
    pub task: NoteId,
    // The outcome, in detail.
    pub result: Body,
    // Everything done, failures included, in brief.
    pub work: Body,
    pub author: Option<Author>,
    pub run: Option<RunId>,
}

fn summary_id(conn: &Connection, task: NoteId) -> Result<Option<NoteId>> {
    Ok(conn
        .query_row(
            "SELECT id FROM notes WHERE parent_id = ?1 AND summary = 1",
            [task],
            |row| row.get(0),
        )
        .optional()?)
}

// Writes or replaces the summary of a task; the old text stays as a revision.
pub fn summarize(
    conn: &mut Connection,
    input: Summarize,
    default_project: ProjectName,
) -> Result<Value> {
    let task = notes::load(conn, input.task)?;
    if !matches!(task.kind, NoteKind::Goal | NoteKind::Step) {
        return Err(invalid(format!(
            "note {} is a {}; a summary closes a goal or a step",
            task.id, task.kind
        )));
    }
    if input.result.as_str().trim().is_empty() || input.work.as_str().trim().is_empty() {
        return Err(invalid("result and work must not be empty"));
    }
    let body = Body::try_from(format!(
        "Result:\n{}\n\nWork done:\n{}",
        input.result.as_str().trim(),
        input.work.as_str().trim()
    ))
    .map_err(invalid)?;

    let (note, created) = match summary_id(conn, task.id)? {
        Some(id) => {
            let patch = NotePatch {
                id,
                title: None,
                body: Some(body),
                status: None,
                tags: None,
                parent: None,
                append: None,
                entity: None,
                author: input.author,
                run: input.run,
                expected_revision: None,
            };
            (notes::update(conn, patch)?, false)
        }
        None => {
            let short: String = task.title.chars().take(MAX_SUMMARY_TITLE_CHARS).collect();
            let new = NewNote {
                kind: NoteKind::Fact,
                title: Title::try_from(format!("Summary: {short}")).map_err(invalid)?,
                body,
                status: None,
                tags: Default::default(),
                parent: Some(task.id),
                project: None,
                links: Default::default(),
                entity: None,
                author: input.author,
                run: input.run,
                task: None,
            };
            let note = notes::create(conn, new, default_project)?.note;
            conn.execute("UPDATE notes SET summary = 1 WHERE id = ?1", [note.id])?;
            (note, true)
        }
    };

    // Finished sub-tasks without a summary.
    let missing = unsummarized(conn, Some(task.id), None, LISTED)?;
    let mut reply = serde_json::to_value(&note).map_err(internal)?;
    reply["summary"] = json!(true);
    reply["created"] = json!(created);
    reply["task"] = json!(task.id);
    if !missing.is_empty() {
        reply["parts_without_summary"] = Value::from(missing);
    }
    Ok(reply)
}

// Finished goals and steps without a summary, under a task or in a project.
fn unsummarized(
    conn: &Connection,
    under: Option<NoteId>,
    project: Option<&str>,
    limit: i64,
) -> Result<Vec<Value>> {
    Ok(conn
        .prepare(
            "WITH RECURSIVE sub (id) AS (
                 SELECT ?1
                 UNION
                 SELECT c.id FROM notes c JOIN sub ON c.parent_id = sub.id
             )
             SELECT n.id, n.kind, n.status, n.title FROM notes n
             WHERE n.kind IN ('goal', 'step') AND n.status IN ('done', 'failed', 'dropped')
               AND (?1 IS NULL OR (n.id IN (SELECT id FROM sub) AND n.id <> ?1))
               AND (?2 IS NULL OR n.project = ?2)
               AND NOT EXISTS (SELECT 1 FROM notes s WHERE s.parent_id = n.id AND s.summary = 1)
             ORDER BY n.updated_at DESC, n.id DESC LIMIT ?3",
        )?
        .query_map(params![under, project, limit], |row| {
            Ok(json!({
                "id": row.get::<_, i64>(0)?,
                "kind": row.get::<_, String>(1)?,
                "status": row.get::<_, String>(2)?,
                "title": row.get::<_, String>(3)?,
            }))
        })?
        .collect::<rusqlite::Result<_>>()?)
}

fn summary_value(
    id: Option<NoteId>,
    body: Option<String>,
    at: Option<String>,
    max: usize,
) -> Value {
    let (Some(id), Some(body)) = (id, body) else {
        return Value::Null;
    };
    let (text, cut) = truncated(&body, max);
    let mut value = json!({ "id": id, "text": text, "updated_at": at });
    if cut {
        value["truncated"] = json!(true);
    }
    value
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TaskSummaryQuery {
    pub task: NoteId,
    pub budget_chars: Option<usize>,
}

// A task as its summaries only: its own, then its sub-tasks' in tree order.
pub fn task_summary(conn: &Connection, query: &TaskSummaryQuery) -> Result<Value> {
    let limit = query.budget_chars.unwrap_or(DEFAULT_BUDGET_CHARS);
    if !(MIN_BUDGET_CHARS..=MAX_BUDGET_CHARS).contains(&limit) {
        return Err(invalid(format!(
            "budget_chars must be between {MIN_BUDGET_CHARS} and {MAX_BUDGET_CHARS}"
        )));
    }
    let task = notes::load(conn, query.task)?;

    let own = conn
        .query_row(
            "SELECT id, body, updated_at FROM notes WHERE parent_id = ?1 AND summary = 1",
            [task.id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .optional()?;
    let own = match own {
        Some((id, body, at)) => summary_value(Some(id), Some(body), Some(at), limit / 2),
        None => Value::Null,
    };
    let mut used = own.to_string().chars().count();

    // Zero padded path sorts rows in depth first order.
    let rows: Vec<Value> = conn
        .prepare(
            "WITH RECURSIVE tree (id, depth, path) AS (
                 SELECT id, 0, printf('%019d', id) FROM notes WHERE id = ?1
                 UNION ALL
                 SELECT c.id, tree.depth + 1, tree.path || printf('/%019d', c.id)
                 FROM notes c JOIN tree ON c.parent_id = tree.id
                 WHERE c.kind IN ('goal', 'step')
             )
             SELECT n.id, n.kind, n.status, n.title, n.parent_id, tree.depth,
                    s.id, s.body, s.updated_at
             FROM tree JOIN notes n ON n.id = tree.id
             LEFT JOIN notes s ON s.parent_id = n.id AND s.summary = 1
             WHERE tree.depth > 0
             ORDER BY tree.path LIMIT ?2",
        )?
        .query_map(params![task.id, SECTION_ROWS], |row| {
            Ok(json!({
                "id": row.get::<_, i64>(0)?,
                "kind": row.get::<_, String>(1)?,
                "status": row.get::<_, String>(2)?,
                "title": row.get::<_, String>(3)?,
                "parent": row.get::<_, Option<i64>>(4)?,
                "depth": row.get::<_, i64>(5)?,
                "summary": summary_value(row.get(6)?, row.get(7)?, row.get(8)?, limit / 4),
            }))
        })?
        .collect::<rusqlite::Result<_>>()?;

    let total = rows.len();
    let mut parts = Vec::new();
    for part in rows {
        let cost = part.to_string().chars().count();
        if used + cost > limit {
            break;
        }
        used += cost;
        parts.push(part);
    }
    let omitted = total - parts.len();
    let without: Vec<&Value> = parts
        .iter()
        .filter(|part| part["summary"].is_null())
        .map(|part| &part["id"])
        .collect();

    Ok(json!({
        "task": {
            "id": task.id,
            "project": task.project,
            "kind": task.kind,
            "status": task.status,
            "title": task.title,
        },
        "summary": own,
        "parts_without_summary": without,
        "parts": parts,
        "checklist": open_items(conn, task.id)?,
        "budget": { "limit": limit, "used": used, "omitted_parts": omitted },
    }))
}

// --------------------------------------------------------------- assessments

validated_string!(
    // One or two sentences on what a confidence rests on.
    Basis,
    validate_basis
);

fn validate_basis(value: String) -> std::result::Result<String, String> {
    let basis = value.trim();
    if basis.is_empty() || basis.chars().count() > MAX_BASIS_CHARS {
        return Err(format!("basis must be 1-{MAX_BASIS_CHARS} characters"));
    }
    if basis.chars().any(char::is_control) {
        return Err("basis must be a single line without control characters".into());
    }
    Ok(basis.to_string())
}

#[derive(Debug, Default)]
pub struct Assessment {
    pub confidence: Option<Confidence>,
    pub basis: Option<Basis>,
}

// Takes `confidence` and `basis` out of the arguments; None when absent.
pub fn take_assessment(args: &mut Value) -> Result<Option<Assessment>> {
    let Some(fields) = args.as_object_mut() else {
        return Ok(None);
    };
    let mut take = |name: &str| fields.remove(name).filter(|value| !value.is_null());
    let (confidence, basis) = (take("confidence"), take("basis"));
    if confidence.is_none() && basis.is_none() {
        return Ok(None);
    }
    let parsed = |problem: serde_json::Error| invalid(problem.to_string());
    Ok(Some(Assessment {
        confidence: confidence
            .map(serde_json::from_value)
            .transpose()
            .map_err(parsed)?,
        basis: basis
            .map(serde_json::from_value)
            .transpose()
            .map_err(parsed)?,
    }))
}

fn record_key(record: RecordType, id: &RecordId) -> Result<String> {
    match (record, id) {
        (RecordType::Note, RecordId::Number(number)) => Ok(number.to_string()),
        (RecordType::Concept, RecordId::Uuid(uuid)) => Ok(uuid.to_string()),
        (RecordType::Note, _) => Err(invalid("a note id is an integer")),
        (RecordType::Concept, _) => Err(invalid("a concept id is a UUID string")),
        (other, _) => Err(invalid(format!(
            "a {other} carries no confidence; use a note or a concept"
        ))),
    }
}

// Confidence, its basis and the last check of one record, or null.
pub fn assessment_of(conn: &Connection, record: RecordType, id: &str) -> Result<Value> {
    Ok(conn
        .query_row(
            "SELECT confidence, basis, verified_at, verified_by FROM assessments
             WHERE record_type = ?1 AND record_id = ?2",
            params![record, id],
            |row| {
                Ok(json!({
                    "confidence": row.get::<_, Option<f64>>(0)?,
                    "basis": row.get::<_, Option<String>>(1)?,
                    "verified_at": row.get::<_, Option<String>>(2)?,
                    "verified_by": row.get::<_, Option<String>>(3)?,
                }))
            },
        )
        .optional()?
        .unwrap_or(Value::Null))
}

// Marks the record as checked now. Fields not given keep their value.
pub fn assess(
    conn: &mut Connection,
    record: RecordType,
    id: &str,
    assessment: &Assessment,
    who: &Attribution,
) -> Result<Value> {
    let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let table = match record {
        RecordType::Note => "SELECT project FROM notes WHERE id = CAST(?1 AS INTEGER)",
        RecordType::Concept => "SELECT project FROM concepts WHERE id = ?1",
        other => {
            return Err(invalid(format!(
                "a {other} carries no confidence; use a note or a concept"
            )));
        }
    };
    let project: String = tx
        .query_row(table, [id], |row| row.get(0))
        .optional()?
        .ok_or_else(|| MemoryError::NotFound(format!("{record} {id}")))?;

    tx.execute(
        "INSERT INTO assessments (record_type, record_id, confidence, basis, verified_at, verified_by)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)
         ON CONFLICT (record_type, record_id) DO UPDATE SET
             confidence = coalesce(excluded.confidence, confidence),
             basis = coalesce(excluded.basis, basis),
             verified_at = excluded.verified_at,
             verified_by = excluded.verified_by",
        params![
            record,
            id,
            assessment.confidence.map(Confidence::value),
            assessment.basis.as_ref().map(Basis::as_str),
            now(),
            who.author,
        ],
    )?;
    let mut fields = vec!["verified"];
    if assessment.confidence.is_some() {
        fields.push("confidence");
    }
    if assessment.basis.is_some() {
        fields.push("basis");
    }
    journal::record(
        &tx,
        Entry::new(&project, record, id, Op::Updated, who).fields(fields),
    )?;
    let saved = assessment_of(&tx, record, id)?;
    tx.commit()?;
    Ok(saved)
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Confirm {
    #[serde(rename = "type")]
    pub record: RecordType,
    pub id: RecordId,
    pub confidence: Option<Confidence>,
    pub basis: Option<Basis>,
    pub author: Option<Author>,
    pub run: Option<RunId>,
}

// Marks a note or concept as checked again and found to hold.
pub fn confirm(conn: &mut Connection, request: Confirm) -> Result<Value> {
    let id = record_key(request.record, &request.id)?;
    let assessment = Assessment {
        confidence: request.confidence,
        basis: request.basis,
    };
    let who = Attribution::new(request.author, request.run, None);
    let mut saved = assess(conn, request.record, &id, &assessment, &who)?;
    saved["type"] = json!(request.record);
    saved["id"] = match request.id {
        RecordId::Number(number) => json!(number),
        RecordId::Uuid(uuid) => json!(uuid.to_string()),
    };
    Ok(saved)
}

// Adds confidence, last check and the summary mark to search results.
pub fn annotate(conn: &Connection, results: &mut [Value]) -> Result<()> {
    for result in results {
        let (record, id) = match (result["source"].as_str(), &result["id"]) {
            (Some("note"), Value::Number(id)) => (RecordType::Note, id.to_string()),
            (Some("concept"), Value::String(id)) => (RecordType::Concept, id.clone()),
            _ => continue,
        };
        let assessment = assessment_of(conn, record, &id)?;
        for field in ["confidence", "verified_at"] {
            if !assessment[field].is_null() {
                result[field] = assessment[field].clone();
            }
        }
        if record == RecordType::Note {
            let summary: Option<bool> = conn
                .query_row(
                    "SELECT summary FROM notes WHERE id = CAST(?1 AS INTEGER)",
                    [&id],
                    |row| row.get(0),
                )
                .optional()?;
            if summary == Some(true) {
                result["summary"] = json!(true);
            }
        }
    }
    Ok(())
}

// -------------------------------------------------------------------- review

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReviewQuery {
    pub project: Option<Scope>,
    pub stale_days: Option<u32>,
}

fn listed(conn: &Connection, filter: &str, project: Option<&str>, cutoff: &str) -> Result<Value> {
    let total: i64 = conn.query_row(
        &format!("SELECT count(*) FROM notes n WHERE (?1 IS NULL OR n.project = ?1) AND {filter}"),
        params![project, cutoff],
        |row| row.get(0),
    )?;
    let oldest: Vec<Value> = conn
        .prepare(&format!(
            "SELECT n.id, n.kind, n.status, n.title, n.updated_at FROM notes n
             WHERE (?1 IS NULL OR n.project = ?1) AND {filter}
             ORDER BY n.updated_at, n.id LIMIT ?3"
        ))?
        .query_map(params![project, cutoff, LISTED], |row| {
            Ok(json!({
                "id": row.get::<_, i64>(0)?,
                "kind": row.get::<_, String>(1)?,
                "status": row.get::<_, String>(2)?,
                "title": row.get::<_, String>(3)?,
                "updated_at": row.get::<_, String>(4)?,
            }))
        })?
        .collect::<rusqlite::Result<_>>()?;
    Ok(json!({ "total": total, "oldest": oldest }))
}

// What in a project needs attention. Reads only; it changes nothing.
pub fn review(conn: &Connection, query: &ReviewQuery, default_scope: &Scope) -> Result<Value> {
    let days = query.stale_days.unwrap_or(DEFAULT_STALE_DAYS);
    if !(1..=MAX_STALE_DAYS).contains(&days) {
        return Err(invalid(format!(
            "stale_days must be between 1 and {MAX_STALE_DAYS}"
        )));
    }
    let project = query.project.as_ref().unwrap_or(default_scope).project();
    let cutoff = seconds_from_now(-i64::from(days) * 86_400);
    let at = now();

    let stalled = listed(
        conn,
        "n.kind IN ('goal', 'step') AND n.status IN ('open', 'active') AND n.updated_at < ?2",
        project,
        &cutoff,
    )?;
    let questions = listed(
        conn,
        "n.kind = 'question' AND n.status = 'open' AND ?2 IS NOT NULL",
        project,
        &cutoff,
    )?;
    let loose = listed(
        conn,
        "n.parent_id IS NULL AND n.kind <> 'goal' AND n.updated_at < ?2",
        project,
        &cutoff,
    )?;

    let (open_items, done_items): (i64, i64) = conn.query_row(
        "SELECT coalesce(sum(1 - i.done), 0), coalesce(sum(i.done), 0)
         FROM checklist_items i JOIN notes n ON n.id = i.note_id
         WHERE (?1 IS NULL OR n.project = ?1)",
        [project],
        |row| Ok((row.get(0)?, row.get(1)?)),
    )?;
    let (held, lapsed): (i64, i64) = conn.query_row(
        "SELECT coalesce(sum(claim_expires_at > ?2), 0), coalesce(sum(claim_expires_at <= ?2), 0)
         FROM entities WHERE claim_id IS NOT NULL AND (?1 IS NULL OR project = ?1)",
        params![project, at],
        |row| Ok((row.get(0)?, row.get(1)?)),
    )?;
    let doubtful: i64 = conn.query_row(
        "SELECT count(*) FROM assessments a
         LEFT JOIN notes n ON a.record_type = 'note' AND n.id = CAST(a.record_id AS INTEGER)
         LEFT JOIN concepts c ON a.record_type = 'concept' AND c.id = a.record_id
         WHERE a.confidence < 0.5 AND (?1 IS NULL OR coalesce(n.project, c.project) = ?1)",
        [project],
        |row| row.get(0),
    )?;
    let unchecked: i64 = conn.query_row(
        "SELECT count(*) FROM concepts c
         WHERE (?1 IS NULL OR c.project = ?1) AND c.archived = 0
           AND NOT EXISTS (SELECT 1 FROM assessments a
                           WHERE a.record_type = 'concept' AND a.record_id = c.id
                             AND a.verified_at >= ?2)",
        params![project, cutoff],
        |row| row.get(0),
    )?;

    let mut report = Map::new();
    report.insert("stale_days".into(), json!(days));
    report.insert("stalled_tasks".into(), stalled);
    report.insert("open_questions".into(), questions);
    report.insert(
        "closed_without_summary".into(),
        Value::from(unsummarized(conn, None, project, LISTED)?),
    );
    report.insert(
        "checklists".into(),
        json!({ "open": open_items, "done": done_items }),
    );
    report.insert("notes_outside_any_task".into(), loose);
    report.insert(
        "claims".into(),
        json!({ "held": held, "expired_not_released": lapsed }),
    );
    report.insert("records_with_confidence_below_half".into(), json!(doubtful));
    report.insert("concepts_not_checked_lately".into(), json!(unchecked));
    Ok(Value::Object(report))
}
