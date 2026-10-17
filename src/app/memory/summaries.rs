// A live summary belongs to one task. Reads begin at that task and expose one
// page of immediate children; deeper work always needs another explicit call.

use rusqlite::{Connection, OptionalExtension, params};
use serde::Deserialize;
use serde_json::{Value, json};

use super::journal::{self, Entry, Op, RecordType};
use super::model::{Attribution, MemoryError, Note, NoteId, NoteKind, invalid, now};
use super::notes;
use crate::settings::resume::{DEFAULT_CHILDREN, MAX_CHILDREN};

type Result<T> = std::result::Result<T, MemoryError>;

pub(super) fn ensure(conn: &Connection, task: &Note, who: &Attribution) -> Result<NoteId> {
    let at = now();
    conn.execute(
        "INSERT OR IGNORE INTO task_summary_state (task_id, updated_at) VALUES (?1, ?2)",
        params![task.id, at],
    )?;
    if let Some(id) = id(conn, task.id)? {
        return Ok(id);
    }
    let title: String = task.title.chars().take(180).collect();
    // The initial brief is useful to the first agent, but is not a reviewed
    // account of the work. reviewed_version stays NULL until summarize.
    let body = if task.body.trim().is_empty() {
        &task.title
    } else {
        &task.body
    };
    conn.execute(
        "INSERT INTO notes (project, kind, status, title, body, tags, parent_id, summary,
                            author, created_at, updated_at)
         VALUES (?1, 'fact', 'active', ?2, ?3, '', ?4, 1, ?5, ?6, ?6)",
        params![
            task.project,
            format!("Summary: {title}"),
            body,
            task.id,
            who.author,
            at
        ],
    )?;
    let summary = notes::load(conn, conn.last_insert_rowid())?;
    notes::save_revision(conn, &summary, who)?;
    journal::record(
        conn,
        Entry::new(
            &summary.project,
            RecordType::Note,
            summary.id,
            Op::Created,
            who,
        )
        .revision(summary.revision)
        .fields(["summary"]),
    )?;
    Ok(summary.id)
}

pub(super) fn id(conn: &Connection, task: NoteId) -> Result<Option<NoteId>> {
    Ok(conn
        .query_row(
            "SELECT id FROM notes WHERE parent_id = ?1 AND summary = 1",
            [task],
            |row| row.get(0),
        )
        .optional()?)
}

pub(super) fn version(conn: &Connection, task: NoteId) -> Result<i64> {
    Ok(conn.query_row(
        "SELECT change_version FROM task_summary_state WHERE task_id = ?1",
        [task],
        |row| row.get(0),
    )?)
}

pub(super) fn mark_reviewed(conn: &Connection, task: NoteId) -> Result<()> {
    conn.execute(
        "UPDATE task_summary_state SET reviewed_version = change_version, reviewed_at = ?2
         WHERE task_id = ?1",
        params![task, now()],
    )?;
    Ok(())
}

// Report IDs are bounded by the deserialized input, verified before the write,
// and retained if omitted. A purged report remains an explicit missing pointer.
pub(super) fn set_reports(
    conn: &Connection,
    summary: NoteId,
    reports: &[NoteId],
    who: &Attribution,
) -> Result<()> {
    let mut unique = reports.to_vec();
    unique.sort_unstable();
    unique.dedup();
    for id in &unique {
        let report = notes::load(conn, *id)?;
        if *id == summary || matches!(report.kind, NoteKind::Goal | NoteKind::Step) {
            return Err(invalid(
                "report_ids must name report notes, not the summary itself or task nodes",
            ));
        }
    }
    let note = notes::load(conn, summary)?;
    if note.report_ids == unique {
        return Ok(());
    }
    conn.execute(
        "UPDATE notes SET report_ids = ?2, revision = revision + 1, updated_at = ?3 WHERE id = ?1",
        params![summary, json!(unique).to_string(), now()],
    )?;
    let note = notes::load(conn, summary)?;
    notes::save_revision(conn, &note, who)?;
    journal::record(
        conn,
        Entry::new(&note.project, RecordType::Note, summary, Op::Updated, who)
            .revision(note.revision)
            .fields(["report_ids"]),
    )?;
    Ok(())
}

pub(super) fn own(conn: &Connection, task: NoteId) -> Result<Value> {
    let Some(summary) = id(conn, task)? else {
        return Ok(Value::Null);
    };
    let note = notes::load(conn, summary)?;
    let (changed, reviewed, changed_at, reviewed_at): (i64, Option<i64>, String, Option<String>) = conn.query_row(
        "SELECT change_version, reviewed_version, updated_at, reviewed_at FROM task_summary_state WHERE task_id = ?1",
        [task], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
    )?;
    let missing: Vec<NoteId> = conn.prepare(
        "SELECT value FROM json_each(?1) WHERE NOT EXISTS (SELECT 1 FROM notes WHERE id = value)",
    )?.query_map([json!(note.report_ids).to_string()], |r| r.get(0))?.collect::<rusqlite::Result<_>>()?;
    Ok(json!({
        "id": summary, "text": note.body, "truncated": false, "revision": note.revision,
        "report_ids": note.report_ids, "missing_report_ids": missing,
        "created_at": note.created_at, "updated_at": note.updated_at,
        "confidence": note.confidence, "basis": note.basis,
        "freshness": { "stale": reviewed != Some(changed) || note.status == super::model::Status::Dropped,
            "initialized": reviewed.is_some(), "change_version": changed,
            "reviewed_version": reviewed, "changed_at": changed_at, "reviewed_at": reviewed_at }
    }))
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Query {
    pub task: NoteId,
    pub children_limit: Option<usize>,
    pub children_cursor: Option<NoteId>,
    pub children_offset: Option<usize>,
}

// Consistent totals, page and summary even with another process writing.
pub fn read(conn: &Connection, query: &Query) -> Result<Value> {
    let tx = conn.unchecked_transaction()?;
    let result = read_in(&tx, query)?;
    tx.commit()?;
    Ok(result)
}

// The summary and its report references come whole. Only the immediate
// child descriptors are paged; child summaries require their own request.
fn read_in(tx: &Connection, query: &Query) -> Result<Value> {
    let page_size = query.children_limit.unwrap_or(DEFAULT_CHILDREN);
    if !(1..=MAX_CHILDREN).contains(&page_size) {
        return Err(invalid(format!(
            "children_limit must be between 1 and {MAX_CHILDREN}"
        )));
    }
    if query.children_cursor.is_some_and(|id| id < 1) {
        return Err(invalid("children_cursor must be a positive child ID"));
    }
    if query.children_cursor.is_some() && query.children_offset.is_some() {
        return Err(invalid("give children_cursor or children_offset, not both"));
    }
    let offset = query.children_offset.unwrap_or(0);
    if offset > 1_000_000 {
        return Err(invalid("children_offset must be at most 1000000"));
    }
    let task = notes::load(tx, query.task)?;
    if !matches!(task.kind, NoteKind::Goal | NoteKind::Step) {
        return Err(invalid("task must be a goal or step"));
    }
    let total: i64 = tx.query_row(
        "SELECT count(*) FROM notes WHERE parent_id = ?1 AND kind IN ('goal', 'step')",
        [task.id],
        |r| r.get(0),
    )?;
    let before: i64 = if let Some(cursor) = query.children_cursor {
        tx.query_row("SELECT count(*) FROM notes WHERE parent_id = ?1 AND kind IN ('goal', 'step') AND id <= ?2", params![task.id, cursor], |r| r.get(0))?
    } else {
        (offset as i64).min(total)
    };
    let children: Vec<Value> = tx
        .prepare(
            "SELECT n.id, n.kind, n.title, n.status, n.updated_at,
                (SELECT s.id FROM notes s WHERE s.parent_id = n.id AND s.summary = 1)
         FROM notes n WHERE n.parent_id = ?1 AND n.kind IN ('goal', 'step') AND n.id > ?2
         ORDER BY n.id LIMIT ?3 OFFSET ?4",
        )?
        .query_map(
            params![
                task.id,
                query.children_cursor.unwrap_or(0),
                page_size as i64,
                offset as i64
            ],
            |r| {
                Ok(
                    json!({"id": r.get::<_, i64>(0)?, "kind": r.get::<_, String>(1)?,
            "title": r.get::<_, String>(2)?, "status": r.get::<_, String>(3)?,
            "updated_at": r.get::<_, String>(4)?, "summary_id": r.get::<_, Option<i64>>(5)? }),
                )
            },
        )?
        .collect::<rusqlite::Result<_>>()?;
    let remaining = total - before - children.len() as i64;
    let next = if remaining > 0 {
        children.last().and_then(|child| child["id"].as_i64())
    } else {
        None
    };
    Ok(json!({
        "task": { "id": task.id, "project": task.project, "kind": task.kind,
            "title": task.title, "status": task.status, "parent": task.parent, "updated_at": task.updated_at },
        "summary": own(tx, task.id)?,
        "children": { "total": total, "offset": before, "returned": children.len(),
            "items": children, "remaining": remaining, "next_cursor": next },
    }))
}
