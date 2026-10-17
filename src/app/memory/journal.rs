// Journal of every change: record, op, field names, author, run, task. It
// holds no record text, so entries outlive a purged record. Notes also keep
// up to `REVISIONS_KEPT` full revisions.

use rusqlite::types::Value as Sql;
use rusqlite::{Connection, OptionalExtension, TransactionBehavior, params, params_from_iter};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use super::db::Db;
use super::model::{
    Attribution, Author, Limit, MemoryError, NoteId, RunId, Scope, invalid, now, sql_text,
    string_enum,
};

pub const REVISIONS_KEPT: i64 = 50;
const DEFAULT_HISTORY_LIMIT: usize = 20;

type Result<T> = std::result::Result<T, MemoryError>;

string_enum!(RecordType {
    Note => "note",
    Entity => "entity",
    Checkpoint => "checkpoint",
});

string_enum!(Op {
    Baseline => "baseline",
    Created => "created",
    Updated => "updated",
    Checked => "checked",
    Claimed => "claimed",
    Released => "released",
    Linked => "linked",
    Unlinked => "unlinked",
    Redacted => "redacted",
    Purged => "purged",
});

string_enum!(Origin {
    Recorded => "recorded",
    LegacyBaseline => "legacy_baseline",
    Backfilled => "backfilled",
});

sql_text!(RecordType, Op, Origin);

pub struct Entry<'a> {
    project: &'a str,
    record: RecordType,
    id: i64,
    op: Op,
    revision: Option<i64>,
    fields: Vec<String>,
    who: &'a Attribution,
    event_id: Option<i64>,
}

impl<'a> Entry<'a> {
    pub fn new(
        project: &'a str,
        record: RecordType,
        id: i64,
        op: Op,
        who: &'a Attribution,
    ) -> Self {
        Self {
            project,
            record,
            id,
            op,
            revision: None,
            fields: Vec::new(),
            who,
            event_id: None,
        }
    }

    pub fn revision(mut self, revision: i64) -> Self {
        self.revision = Some(revision);
        self
    }

    pub fn fields<S: Into<String>>(mut self, fields: impl IntoIterator<Item = S>) -> Self {
        self.fields = fields.into_iter().map(Into::into).collect();
        self
    }

    pub fn event(mut self, event_id: i64) -> Self {
        self.event_id = Some(event_id);
        self
    }
}

// Appends an entry. Call inside the transaction that makes the change.
pub fn record(conn: &Connection, entry: Entry<'_>) -> Result<i64> {
    conn.execute(
        "INSERT INTO journal
             (project, record_type, record_id, op, revision, fields, actor, run, task_id,
              event_id, origin, at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, 'recorded', ?11)",
        params![
            entry.project,
            entry.record,
            entry.id,
            entry.op,
            entry.revision,
            Value::from(entry.fields).to_string(),
            entry.who.author,
            entry.who.run,
            entry.who.task,
            entry.event_id,
            now(),
        ],
    )?;
    Ok(conn.last_insert_rowid())
}

pub struct NoteState<'a> {
    pub id: NoteId,
    pub revision: i64,
    pub status: &'a str,
    pub title: &'a str,
    pub body: &'a str,
    pub tags: &'a str,
    pub parent: Option<NoteId>,
    pub entity: Option<i64>,
    pub confidence: Option<f64>,
    pub basis: Option<&'a str>,
}

pub fn save_note_revision(
    conn: &Connection,
    state: &NoteState<'_>,
    who: &Attribution,
) -> Result<()> {
    conn.execute(
        "INSERT INTO note_revisions
             (note_id, revision, status, title, body, tags, parent_id, entity_id, confidence,
              basis, actor, run, origin, at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, 'recorded', ?13)",
        params![
            state.id,
            state.revision,
            state.status,
            state.title,
            state.body,
            state.tags,
            state.parent,
            state.entity,
            state.confidence,
            state.basis,
            who.author,
            who.run,
            now(),
        ],
    )?;
    conn.execute(
        "DELETE FROM note_revisions WHERE note_id = ?1 AND revision <= ?2",
        params![state.id, state.revision - REVISIONS_KEPT],
    )?;
    Ok(())
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HistoryQuery {
    #[serde(rename = "type")]
    pub record: Option<RecordType>,
    pub id: Option<i64>,
    pub project: Option<Scope>,
    pub task: Option<NoteId>,
    pub cursor: Option<i64>,
    pub limit: Option<Limit>,
}

#[derive(Debug, Serialize)]
pub struct HistoryEntry {
    pub seq: i64,
    pub at: String,
    pub project: String,
    #[serde(rename = "type")]
    pub record: RecordType,
    pub id: i64,
    pub op: Op,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub revision: Option<i64>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub fields: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub author: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub run: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub task: Option<NoteId>,
    pub origin: Origin,
    // Current name of the record; absent once it is deleted.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    // What an entity event changed; absent once the entity is deleted.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<Value>,
}

#[derive(Debug, Serialize)]
pub struct History {
    pub entries: Vec<HistoryEntry>,
    // Pass as `cursor` for the next, older page; null when there is none.
    pub next_cursor: Option<i64>,
}

const ENTRY_SELECT: &str = "\
    SELECT j.id, j.at, j.project, j.record_type, j.record_id, j.op, j.revision, j.fields,
           j.actor, j.run, j.task_id, j.origin, coalesce(n.title, e.type || ':' || e.key),
           v.detail
    FROM journal j
    LEFT JOIN notes n ON j.record_type = 'note' AND n.id = j.record_id
    LEFT JOIN entities e ON j.record_type = 'entity' AND e.id = j.record_id
    LEFT JOIN entity_events v ON v.id = j.event_id AND j.record_type = 'entity'";

// Entries attributed to a task, or made to the notes beneath it.
const IN_TASK: &str = "\
    (j.task_id = ?
     OR (j.record_type = 'note' AND j.record_id IN (
         WITH RECURSIVE sub (id) AS (
             SELECT ?
             UNION
             SELECT c.id FROM notes c JOIN sub ON c.parent_id = sub.id
         )
         SELECT id FROM sub)))";

fn entry_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<HistoryEntry> {
    let fields: String = row.get(7)?;
    let detail: Option<String> = row.get(13)?;
    Ok(HistoryEntry {
        seq: row.get(0)?,
        at: row.get(1)?,
        project: row.get(2)?,
        record: row.get(3)?,
        id: row.get(4)?,
        op: row.get(5)?,
        revision: row.get(6)?,
        fields: serde_json::from_str(&fields).unwrap_or_default(),
        author: row.get(8)?,
        run: row.get(9)?,
        task: row.get(10)?,
        origin: row.get(11)?,
        label: row.get(12)?,
        detail: detail.and_then(|text| serde_json::from_str(&text).ok()),
    })
}

// Newest first. New entries never shift an older page.
pub fn history(conn: &Connection, query: &HistoryQuery, default_scope: &Scope) -> Result<History> {
    let limit = Limit::or(query.limit, DEFAULT_HISTORY_LIMIT);
    let mut sql = format!("{ENTRY_SELECT} WHERE TRUE");
    let mut values: Vec<Sql> = Vec::new();

    match (&query.record, query.id) {
        (Some(record), Some(id)) => {
            sql.push_str(" AND j.record_type = ? AND j.record_id = ?");
            values.push(Sql::Text(record.as_str().into()));
            values.push(Sql::Integer(id));
        }
        (None, Some(_)) => return Err(invalid("`id` needs `type`")),
        (record, None) => {
            // A listing stays inside a project; a single record does not need one.
            if let Some(project) = query.project.as_ref().unwrap_or(default_scope).project() {
                sql.push_str(" AND j.project = ?");
                values.push(Sql::Text(project.into()));
            }
            if let Some(record) = record {
                sql.push_str(" AND j.record_type = ?");
                values.push(Sql::Text(record.as_str().into()));
            }
        }
    }
    if let Some(task) = query.task {
        sql.push_str(" AND ");
        sql.push_str(IN_TASK);
        values.extend([Sql::Integer(task), Sql::Integer(task)]);
    }
    if let Some(cursor) = query.cursor {
        if cursor < 1 {
            return Err(invalid("cursor must be a next_cursor from an earlier page"));
        }
        sql.push_str(" AND j.id < ?");
        values.push(Sql::Integer(cursor));
    }
    sql.push_str(" ORDER BY j.id DESC LIMIT ?");
    values.push(Sql::Integer(limit as i64 + 1));

    let mut entries: Vec<HistoryEntry> = conn
        .prepare(&sql)?
        .query_map(params_from_iter(values), entry_from_row)?
        .collect::<rusqlite::Result<_>>()?;
    let more = entries.len() > limit;
    entries.truncate(limit);
    let next_cursor = entries.last().filter(|_| more).map(|entry| entry.seq);
    Ok(History {
        entries,
        next_cursor,
    })
}

// A task's entries after a journal position, newest first, with their count.
pub fn task_changes(
    conn: &Connection,
    task: NoteId,
    after: i64,
    limit: usize,
) -> Result<(i64, Vec<HistoryEntry>)> {
    let total = conn.query_row(
        &format!("SELECT count(*) FROM journal j WHERE j.id > ?1 AND {IN_TASK}"),
        params![after, task, task],
        |row| row.get(0),
    )?;
    if limit == 0 {
        return Ok((total, Vec::new()));
    }
    let entries = conn
        .prepare(&format!(
            "{ENTRY_SELECT} WHERE j.id > ?1 AND {IN_TASK} ORDER BY j.id DESC LIMIT ?4"
        ))?
        .query_map(params![after, task, task, limit as i64], entry_from_row)?
        .collect::<rusqlite::Result<_>>()?;
    Ok((total, entries))
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RevisionRequest {
    pub id: NoteId,
    pub revision: Option<i64>,
}

// The state a note had at a revision; the current one when none is named.
pub fn revision(conn: &Connection, request: &RevisionRequest) -> Result<Value> {
    let id = request.id;
    let current: i64 = conn
        .query_row("SELECT revision FROM notes WHERE id = ?1", [id], |row| {
            row.get(0)
        })
        .optional()?
        .ok_or_else(|| MemoryError::NotFound(format!("note {id}")))?;
    let wanted = request.revision.unwrap_or(current);
    conn.query_row(
        "SELECT revision, origin, at, actor, run, status, title, body, tags, parent_id,
                entity_id, confidence, basis
         FROM note_revisions WHERE note_id = ?1 AND revision = ?2",
        params![id, wanted],
        |row| {
            let tags: String = row.get(8)?;
            let revision: i64 = row.get(0)?;
            Ok(json!({
                "id": id,
                "revision": revision,
                "current": revision == current,
                "origin": row.get::<_, Origin>(1)?,
                "at": row.get::<_, String>(2)?,
                "author": row.get::<_, Option<String>>(3)?,
                "run": row.get::<_, Option<String>>(4)?,
                "state": {
                    "status": row.get::<_, String>(5)?,
                    "title": row.get::<_, String>(6)?,
                    "body": row.get::<_, String>(7)?,
                    "tags": tags.split_whitespace().collect::<Vec<_>>(),
                    "parent": row.get::<_, Option<i64>>(9)?,
                    "entity": row.get::<_, Option<i64>>(10)?,
                    "confidence": row.get::<_, Option<f64>>(11)?,
                    "basis": row.get::<_, Option<String>>(12)?,
                },
            }))
        },
    )
    .optional()?
    .ok_or_else(|| {
        MemoryError::NotFound(format!(
            "revision {wanted} of note {id} (the latest is {current}; older ones may have been \
             redacted or dropped by retention)"
        ))
    })
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RedactRequest {
    pub id: NoteId,
    pub author: Option<Author>,
    pub run: Option<RunId>,
}

// Deletes every revision of a note but the current one. Journal entries stay.
pub fn redact(conn: &mut Connection, request: RedactRequest) -> Result<i64> {
    let id = request.id;
    let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let (project, current): (String, i64) = tx
        .query_row(
            "SELECT project, revision FROM notes WHERE id = ?1",
            [id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()?
        .ok_or_else(|| MemoryError::NotFound(format!("note {id}")))?;
    let removed = tx.execute(
        "DELETE FROM note_revisions WHERE note_id = ?1 AND revision < ?2",
        params![id, current],
    )? as i64;
    if removed > 0 {
        let who = Attribution::new(request.author, request.run, None);
        record(
            &tx,
            Entry::new(&project, RecordType::Note, id, Op::Redacted, &who).revision(current),
        )?;
    }
    tx.commit()?;
    Ok(removed)
}

#[derive(Clone)]
pub struct Journal {
    db: Db,
}

impl Journal {
    pub fn new(db: Db) -> Self {
        Self { db }
    }

    pub async fn history(&self, query: HistoryQuery, default_scope: Scope) -> Result<History> {
        self.db
            .run(move |conn| history(conn, &query, &default_scope))
            .await
    }

    pub async fn revision(&self, request: RevisionRequest) -> Result<Value> {
        self.db.run(move |conn| revision(conn, &request)).await
    }

    pub async fn redact(&self, request: RedactRequest) -> Result<i64> {
        self.db.run(move |conn| redact(conn, request)).await
    }
}
