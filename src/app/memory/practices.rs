// Checklists (done or not), one summary per task, confirming a note, and a
// review of what needs attention.

use rusqlite::{Connection, OptionalExtension, TransactionBehavior, params};
use serde::Deserialize;
use serde_json::{Map, Value, json};

use super::journal::{self, Entry, NoteState, Op, RecordType};
use super::model::{
    AtMost, Attribution, Author, Basis, Body, Confidence, MemoryError, NewNote, NoteId, NoteKind,
    NotePatch, ProjectName, RunId, Scope, Status, Title, invalid, now, seconds_from_now,
};
use super::notes;
use super::tasks::{DEFAULT_BUDGET_CHARS, MAX_BUDGET_CHARS, MIN_BUDGET_CHARS, SUBTREE, truncated};

const MAX_ITEMS_PER_NOTE: i64 = 100;
const MAX_SUMMARY_TITLE_CHARS: usize = 180;
const LISTED: i64 = 20;
const SECTION_ROWS: i64 = 200;
const DEFAULT_STALE_DAYS: u32 = 14;
const MAX_STALE_DAYS: u32 = 365;

type Result<T> = std::result::Result<T, MemoryError>;

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

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Summarize {
    pub task: NoteId,
    // The outcome, in detail.
    pub result: Body,
    // Everything done, failures included, in brief.
    pub work: Body,
    // What the task ends as; `done` unless said otherwise.
    pub status: Option<Status>,
    pub confidence: Option<Confidence>,
    pub basis: Option<Basis>,
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

fn patch(id: NoteId, author: Option<Author>, run: Option<RunId>) -> NotePatch {
    NotePatch {
        id,
        title: None,
        body: None,
        status: None,
        tags: None,
        parent: None,
        append: None,
        entity: None,
        author,
        run,
        expected_revision: None,
        confidence: None,
        basis: None,
    }
}

// Writes or replaces the summary of a task and closes the task. The old
// summary text stays as a revision.
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
    let closed_as = input.status.unwrap_or(Status::Done);
    if !matches!(closed_as, Status::Done | Status::Failed | Status::Dropped) {
        return Err(invalid("status must be done, failed or dropped"));
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
            let change = NotePatch {
                body: Some(body),
                confidence: input.confidence,
                basis: input.basis,
                ..patch(id, input.author.clone(), input.run.clone())
            };
            (notes::update(conn, change)?, false)
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
                author: input.author.clone(),
                run: input.run.clone(),
                task: None,
                confidence: input.confidence,
                basis: input.basis,
            };
            let mut note = notes::create(conn, new, default_project)?.note;
            conn.execute("UPDATE notes SET summary = 1 WHERE id = ?1", [note.id])?;
            note.summary = true;
            (note, true)
        }
    };
    if task.status != closed_as {
        let close = NotePatch {
            status: Some(closed_as),
            ..patch(task.id, input.author, input.run)
        };
        notes::update(conn, close)?;
    }

    let mut reply = serde_json::to_value(&note).map_err(|e| MemoryError::Internal(e.into()))?;
    reply["created"] = json!(created);
    reply["task"] = json!({ "id": task.id, "status": closed_as });
    let open = open_items(conn, task.id)?;
    if open["open"]
        .as_array()
        .is_some_and(|items| !items.is_empty())
    {
        reply["checklist_left_open"] = open["open"].clone();
    }
    let missing = unsummarized(conn, Some(task.id), None, LISTED)?;
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

// One summary as the reader sees it: text, and how far it was trusted.
struct SummaryRow {
    id: Option<NoteId>,
    body: Option<String>,
    updated_at: Option<String>,
    confidence: Option<f64>,
    basis: Option<String>,
}

impl SummaryRow {
    fn read(row: &rusqlite::Row<'_>, first: usize) -> rusqlite::Result<Self> {
        Ok(Self {
            id: row.get(first)?,
            body: row.get(first + 1)?,
            updated_at: row.get(first + 2)?,
            confidence: row.get(first + 3)?,
            basis: row.get(first + 4)?,
        })
    }

    fn shown(self, max_chars: usize) -> Value {
        let (Some(id), Some(body)) = (self.id, self.body) else {
            return Value::Null;
        };
        let (text, cut) = truncated(&body, max_chars);
        let mut value = json!({ "id": id, "text": text, "updated_at": self.updated_at });
        if cut {
            value["truncated"] = json!(true);
        }
        if let Some(confidence) = self.confidence {
            value["confidence"] = json!(confidence);
        }
        if let Some(basis) = self.basis {
            value["basis"] = json!(basis);
        }
        value
    }
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
            "SELECT id, body, updated_at, confidence, basis FROM notes
             WHERE parent_id = ?1 AND summary = 1",
            [task.id],
            |row| SummaryRow::read(row, 0),
        )
        .optional()?
        .map_or(Value::Null, |summary| summary.shown(limit / 2));
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
                    s.id, s.body, s.updated_at, s.confidence, s.basis
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
                "summary": SummaryRow::read(row, 6)?.shown(limit / 4),
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

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Confirm {
    pub id: NoteId,
    pub confidence: Option<Confidence>,
    pub basis: Option<Basis>,
    pub author: Option<Author>,
    pub run: Option<RunId>,
}

// Marks a note as checked again now and found to hold. This is the only
// way `verified_at` is set. A new confidence or basis makes a new revision,
// so the earlier values stay readable.
pub fn confirm(conn: &mut Connection, request: Confirm) -> Result<Value> {
    let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let current = notes::load(&tx, request.id)?;
    let who = Attribution::new(request.author, request.run, None);
    let confidence = request
        .confidence
        .map(Confidence::value)
        .or(current.confidence);
    let basis = request
        .basis
        .map(String::from)
        .or_else(|| current.basis.clone());

    let mut fields = vec!["verified"];
    if confidence != current.confidence {
        fields.push("confidence");
    }
    if basis != current.basis {
        fields.push("basis");
    }
    let revised = fields.len() > 1;
    let revision = current.revision + i64::from(revised);
    tx.execute(
        "UPDATE notes SET confidence = ?2, basis = ?3, verified_at = ?4, verified_by = ?5,
                          revision = ?6
         WHERE id = ?1",
        params![current.id, confidence, basis, now(), who.author, revision],
    )?;
    if revised {
        journal::save_note_revision(
            &tx,
            &NoteState {
                id: current.id,
                revision,
                status: current.status.as_str(),
                title: &current.title,
                body: &current.body,
                tags: &current.tags.join(" "),
                parent: current.parent,
                entity: current.entity,
                confidence,
                basis: basis.as_deref(),
            },
            &who,
        )?;
    }
    journal::record(
        &tx,
        Entry::new(
            &current.project,
            RecordType::Note,
            current.id,
            Op::Updated,
            &who,
        )
        .revision(revision)
        .fields(fields),
    )?;
    let note = notes::load(&tx, current.id)?;
    tx.commit()?;
    serde_json::to_value(&note).map_err(|e| MemoryError::Internal(e.into()))
}

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
            "SELECT n.id, n.kind, n.status, n.title, n.updated_at, n.confidence FROM notes n
             WHERE (?1 IS NULL OR n.project = ?1) AND {filter}
             ORDER BY n.updated_at, n.id LIMIT ?3"
        ))?
        .query_map(params![project, cutoff, LISTED], |row| {
            let mut note = json!({
                "id": row.get::<_, i64>(0)?,
                "kind": row.get::<_, String>(1)?,
                "status": row.get::<_, String>(2)?,
                "title": row.get::<_, String>(3)?,
                "updated_at": row.get::<_, String>(4)?,
            });
            if let Some(confidence) = row.get::<_, Option<f64>>(5)? {
                note["confidence"] = json!(confidence);
            }
            Ok(note)
        })?
        .collect::<rusqlite::Result<_>>()?;
    Ok(json!({ "total": total, "oldest": oldest }))
}

// What in a project needs attention. Reads only.
pub fn review(conn: &Connection, query: &ReviewQuery, default_scope: &Scope) -> Result<Value> {
    let days = query.stale_days.unwrap_or(DEFAULT_STALE_DAYS);
    if !(1..=MAX_STALE_DAYS).contains(&days) {
        return Err(invalid(format!(
            "stale_days must be between 1 and {MAX_STALE_DAYS}"
        )));
    }
    let project = query.project.as_ref().unwrap_or(default_scope).project();
    let cutoff = seconds_from_now(-i64::from(days) * 86_400);
    let section = |filter: &str| listed(conn, filter, project, &cutoff);

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
        params![project, now()],
        |row| Ok((row.get(0)?, row.get(1)?)),
    )?;

    let mut report = Map::new();
    report.insert("stale_days".into(), json!(days));
    report.insert(
        "stalled_tasks".into(),
        section(
            "n.kind IN ('goal', 'step') AND n.status IN ('open', 'active') AND n.updated_at < ?2",
        )?,
    );
    report.insert(
        "open_questions".into(),
        section("n.kind = 'question' AND n.status = 'open' AND ?2 IS NOT NULL")?,
    );
    report.insert(
        "closed_without_summary".into(),
        Value::from(unsummarized(conn, None, project, LISTED)?),
    );
    report.insert(
        "checklists".into(),
        json!({ "open": open_items, "done": done_items }),
    );
    report.insert(
        "notes_outside_any_task".into(),
        section("n.parent_id IS NULL AND n.kind NOT IN ('goal', 'lesson') AND n.updated_at < ?2")?,
    );
    report.insert(
        "low_confidence".into(),
        section("n.confidence < 0.5 AND n.status <> 'dropped' AND ?2 IS NOT NULL")?,
    );
    report.insert(
        "lessons_not_confirmed_lately".into(),
        section(
            "n.kind = 'lesson' AND n.status = 'active'
             AND coalesce(n.verified_at, n.created_at) < ?2",
        )?,
    );
    report.insert(
        "claims".into(),
        json!({ "held": held, "expired_not_released": lapsed }),
    );
    Ok(Value::Object(report))
}
