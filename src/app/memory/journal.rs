// Journal of every change: record, op, field names, author, run, task. It
// holds no record text, so entries outlive a purged record. Notes and concepts
// also keep up to `REVISIONS_KEPT` full revisions.

use rusqlite::types::Value as Sql;
use rusqlite::{Connection, OptionalExtension, TransactionBehavior, params, params_from_iter};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::concepts::ConceptId;
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
    Concept => "concept",
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
    Archived => "archived",
    Restored => "restored",
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
    pub project: &'a str,
    pub record: RecordType,
    pub id: String,
    pub op: Op,
    pub revision: Option<i64>,
    pub fields: Vec<String>,
    pub who: &'a Attribution,
    pub event_id: Option<i64>,
    pub origin: Origin,
}

impl<'a> Entry<'a> {
    pub fn new(
        project: &'a str,
        record: RecordType,
        id: impl ToString,
        op: Op,
        who: &'a Attribution,
    ) -> Self {
        Self {
            project,
            record,
            id: id.to_string(),
            op,
            revision: None,
            fields: Vec::new(),
            who,
            event_id: None,
            origin: Origin::Recorded,
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

    pub fn origin(mut self, origin: Origin) -> Self {
        self.origin = origin;
        self
    }
}

// Appends an entry. Call inside the transaction that makes the change.
pub fn record(conn: &Connection, entry: Entry<'_>) -> Result<i64> {
    conn.execute(
        "INSERT INTO journal
             (project, record_type, record_id, op, revision, fields, actor, run, task_id,
              event_id, origin, at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
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
            entry.origin,
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
}

pub fn save_note_revision(
    conn: &Connection,
    state: &NoteState<'_>,
    who: &Attribution,
) -> Result<()> {
    conn.execute(
        "INSERT INTO note_revisions
             (note_id, revision, status, title, body, tags, parent_id, entity_id, actor, run, origin, at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, 'recorded', ?11)",
        params![
            state.id,
            state.revision,
            state.status,
            state.title,
            state.body,
            state.tags,
            state.parent,
            state.entity,
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

pub struct ConceptState<'a> {
    pub id: &'a str,
    pub revision: i64,
    pub title: &'a str,
    pub content: &'a str,
    pub tags: &'a str,
    pub sources: &'a str,
    pub archived: bool,
}

pub fn save_concept_revision(
    conn: &Connection,
    state: &ConceptState<'_>,
    who: &Attribution,
    origin: Origin,
    at: &str,
) -> Result<()> {
    conn.execute(
        "INSERT INTO concept_revisions
             (concept_id, revision, title, content, tags, sources, archived, actor, run, origin, at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
        params![
            state.id,
            state.revision,
            state.title,
            state.content,
            state.tags,
            state.sources,
            state.archived,
            who.author,
            who.run,
            origin,
            at,
        ],
    )?;
    conn.execute(
        "DELETE FROM concept_revisions WHERE concept_id = ?1 AND revision <= ?2",
        params![state.id, state.revision - REVISIONS_KEPT],
    )?;
    Ok(())
}

// Queues a vector for removal from the search index.
pub fn unindex(conn: &Connection, record: RecordType, id: impl ToString) -> Result<()> {
    conn.execute(
        "INSERT OR IGNORE INTO index_deletions (record_type, record_id) VALUES (?1, ?2)",
        params![record, id.to_string()],
    )?;
    Ok(())
}

// A note id or a concept id, as the caller wrote it.
#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
pub enum RecordId {
    Number(i64),
    Uuid(ConceptId),
}

impl RecordId {
    fn text(&self) -> String {
        match self {
            Self::Number(id) => id.to_string(),
            Self::Uuid(id) => id.to_string(),
        }
    }

    fn check(&self, record: RecordType) -> Result<()> {
        let fits = match self {
            Self::Number(_) => record != RecordType::Concept,
            Self::Uuid(_) => record == RecordType::Concept,
        };
        if fits {
            Ok(())
        } else {
            Err(invalid(format!(
                "a {record} id is {}",
                if record == RecordType::Concept {
                    "a UUID string"
                } else {
                    "an integer"
                }
            )))
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HistoryQuery {
    #[serde(rename = "type")]
    pub record: Option<RecordType>,
    pub id: Option<RecordId>,
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
    pub id: Value,
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
           j.actor, j.run, j.task_id, j.origin,
           coalesce(n.title, c.title, e.type || ':' || e.key), v.detail
    FROM journal j
    LEFT JOIN notes n ON j.record_type = 'note' AND n.id = CAST(j.record_id AS INTEGER)
    LEFT JOIN concepts c ON j.record_type = 'concept' AND c.id = j.record_id
    LEFT JOIN entities e ON j.record_type = 'entity' AND e.id = CAST(j.record_id AS INTEGER)
    LEFT JOIN entity_events v ON v.id = j.event_id AND j.record_type = 'entity'";

// Entries attributed to a task, or made to the notes beneath it.
pub const IN_TASK: &str = "\
    (j.task_id = ?
     OR (j.record_type = 'note' AND CAST(j.record_id AS INTEGER) IN (
         WITH RECURSIVE sub (id) AS (
             SELECT ?
             UNION
             SELECT c.id FROM notes c JOIN sub ON c.parent_id = sub.id
         )
         SELECT id FROM sub)))";

fn entry_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<HistoryEntry> {
    let record: RecordType = row.get(3)?;
    let id: String = row.get(4)?;
    let fields: String = row.get(7)?;
    let detail: Option<String> = row.get(13)?;
    Ok(HistoryEntry {
        seq: row.get(0)?,
        at: row.get(1)?,
        project: row.get(2)?,
        record,
        id: match (record, id.parse::<i64>()) {
            (RecordType::Concept, _) | (_, Err(_)) => Value::from(id),
            (_, Ok(number)) => Value::from(number),
        },
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

    match (&query.record, &query.id) {
        (Some(record), Some(id)) => {
            id.check(*record)?;
            sql.push_str(" AND j.record_type = ? AND j.record_id = ?");
            values.push(Sql::Text(record.as_str().into()));
            values.push(Sql::Text(id.text()));
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
    #[serde(rename = "type")]
    pub record: RecordType,
    pub id: RecordId,
    pub revision: Option<i64>,
}

#[derive(Debug, Serialize)]
pub struct Revision {
    #[serde(rename = "type")]
    pub record: RecordType,
    pub id: Value,
    pub revision: i64,
    // False when this is an earlier state of the record.
    pub current: bool,
    pub origin: Origin,
    pub at: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub author: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub run: Option<String>,
    pub state: Value,
}

fn space_separated(text: &str) -> Vec<&str> {
    text.split_whitespace().collect()
}

pub fn revision(conn: &Connection, request: &RevisionRequest) -> Result<Revision> {
    request.id.check(request.record)?;
    let id = request.id.text();
    let (table, key, live, columns) = match request.record {
        RecordType::Note => (
            "note_revisions",
            "note_id",
            "notes",
            "json_object('status', r.status, 'title', r.title, 'body', r.body, 'tags', r.tags,
                         'parent', r.parent_id, 'entity', r.entity_id)",
        ),
        RecordType::Concept => (
            "concept_revisions",
            "concept_id",
            "concepts",
            "json_object('title', r.title, 'content', r.content, 'tags', r.tags,
                         'sources', json(r.sources), 'archived', json(iif(r.archived, 'true', 'false')))",
        ),
        other => {
            return Err(invalid(format!(
                "a {other} has no revisions; its changes are listed by `history`"
            )));
        }
    };
    let current: i64 = conn
        .query_row(
            &format!("SELECT revision FROM {live} WHERE id = ?1"),
            [&id],
            |row| row.get(0),
        )
        .optional()?
        .ok_or_else(|| MemoryError::NotFound(format!("{} {id}", request.record)))?;
    let wanted = request.revision.unwrap_or(current);

    let found = conn
        .query_row(
            &format!(
                "SELECT r.revision, r.origin, r.at, r.actor, r.run, {columns}
                 FROM {table} r WHERE r.{key} = ?1 AND r.revision = ?2"
            ),
            params![id, wanted],
            |row| {
                let state: String = row.get(5)?;
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, Origin>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, Option<String>>(3)?,
                    row.get::<_, Option<String>>(4)?,
                    state,
                ))
            },
        )
        .optional()?;
    let Some((revision, origin, at, author, run, state)) = found else {
        return Err(MemoryError::NotFound(format!(
            "revision {wanted} of {} {id} (the latest is {current}; older ones may have been \
             redacted or dropped by retention)",
            request.record
        )));
    };
    let mut state: Value =
        serde_json::from_str(&state).map_err(|error| MemoryError::Internal(error.into()))?;
    if let Some(tags) = state.get("tags").and_then(Value::as_str) {
        state["tags"] = Value::from(space_separated(tags));
    }
    Ok(Revision {
        record: request.record,
        id: match &request.id {
            RecordId::Number(number) => Value::from(*number),
            RecordId::Uuid(uuid) => Value::from(uuid.to_string()),
        },
        revision,
        current: revision == current,
        origin,
        at,
        author,
        run,
        state,
    })
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RedactRequest {
    #[serde(rename = "type")]
    pub record: RecordType,
    pub id: RecordId,
    pub author: Option<Author>,
    pub run: Option<RunId>,
}

// Deletes every revision but the current one. Journal entries stay.
pub fn redact(conn: &mut Connection, request: RedactRequest) -> Result<i64> {
    request.id.check(request.record)?;
    let id = request.id.text();
    let (table, key, live) = match request.record {
        RecordType::Note => ("note_revisions", "note_id", "notes"),
        RecordType::Concept => ("concept_revisions", "concept_id", "concepts"),
        other => return Err(invalid(format!("a {other} has no revisions to redact"))),
    };
    let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let (project, current): (String, i64) = tx
        .query_row(
            &format!("SELECT project, revision FROM {live} WHERE id = ?1"),
            [&id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()?
        .ok_or_else(|| MemoryError::NotFound(format!("{} {id}", request.record)))?;
    let removed = tx.execute(
        &format!("DELETE FROM {table} WHERE {key} = ?1 AND revision < ?2"),
        params![id, current],
    )? as i64;
    if removed > 0 {
        let who = Attribution::new(request.author, request.run, None);
        record(
            &tx,
            Entry::new(&project, request.record, &id, Op::Redacted, &who).revision(current),
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

    pub async fn revision(&self, request: RevisionRequest) -> Result<Revision> {
        self.db.run(move |conn| revision(conn, &request)).await
    }

    pub async fn redact(&self, request: RedactRequest) -> Result<i64> {
        self.db.run(move |conn| redact(conn, request)).await
    }
}
