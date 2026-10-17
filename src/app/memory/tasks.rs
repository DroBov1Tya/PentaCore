// Resuming the area assigned to an agent: the summary of its root task, one
// page of the immediate children (each labelled with kind, title and status,
// never expanded), the last checkpoint, open checklist items and the context
// handed down with `uses`. Nothing is cut by size; lists are bounded by rows.

use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use super::journal;
use super::model::{EdgeKind, MemoryError, NoteBrief, NoteId};
use super::notes::{BRIEF_COLUMNS, CHECKPOINT_COLUMNS, brief_from_row, checkpoint_from_row, load};
use super::{practices, recall, summaries};
use crate::settings::resume::{SECTION_ROWS, TRUNCATION_MARK, WHOLE_TEXT};
use crate::settings::storage::MAX_TREE_DEPTH;

type Result<T> = std::result::Result<T, MemoryError>;

pub(super) const SUBTREE: &str = "\
    WITH RECURSIVE sub (id) AS (
        SELECT ?1
        UNION
        SELECT c.id FROM notes c JOIN sub ON c.parent_id = sub.id
    )
    SELECT id FROM sub";

// The task and every note above it; `distance` is 0 for the task itself.
// Moves that would close a cycle are refused, so the bound only stops a
// damaged tree from looping.
fn lineage() -> String {
    format!(
        "WITH RECURSIVE up (id, distance) AS (
             SELECT ?1, 0
             UNION ALL
             SELECT n.parent_id, up.distance + 1 FROM notes n JOIN up ON n.id = up.id
             WHERE n.parent_id IS NOT NULL AND up.distance < {MAX_TREE_DEPTH}
         )"
    )
}

// A note with its body cut to fit.
#[derive(Serialize)]
struct Excerpt {
    #[serde(flatten)]
    note: NoteBrief,
    #[serde(skip_serializing_if = "String::is_empty")]
    body: String,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    body_truncated: bool,
}

impl Excerpt {
    fn new(note: NoteBrief, body: &str, max_chars: usize) -> Self {
        let (body, body_truncated) = truncated(body, max_chars);
        Self {
            note,
            body,
            body_truncated,
        }
    }
}

// A note handed down with `uses`, how far to trust it, and what replaces or
// disputes it, as `recall` reports.
#[derive(Serialize)]
struct SharedNote {
    #[serde(flatten)]
    excerpt: Excerpt,
    #[serde(skip_serializing_if = "Option::is_none")]
    basis: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    verified_at: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    superseded_by: Vec<NoteId>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    contradicted_by: Vec<NoteId>,
    // The task that shared it: this one, or the nearest one above.
    via: NoteId,
}

pub(super) fn truncated(text: &str, max_chars: usize) -> (String, bool) {
    if text.chars().count() <= max_chars {
        return (text.to_string(), false);
    }
    let kept: String = text.chars().take(max_chars).collect();
    (format!("{kept}{TRUNCATION_MARK}"), true)
}

// Notes shared with `uses` by the task or any task above it, nearest first,
// and how many there are in all. A note shared at several levels is listed
// once, as shared by the nearest. Dropped notes, and notes on the task's own
// line of ancestry, are left out.
fn shared(conn: &Connection, task: NoteId, max_chars: usize) -> Result<(i64, Vec<SharedNote>)> {
    let mut total = 0;
    let mut notes: Vec<SharedNote> = conn
        .prepare(&format!(
            "{lineage},
             nearest (id, via, distance) AS (
                 -- With a single min(), SQLite takes the bare column `via`
                 -- from the row that holds the minimum.
                 SELECT e.dst, e.src, min(up.distance)
                 FROM up JOIN edges e ON e.src = up.id AND e.kind = 'uses'
                 GROUP BY e.dst
             )
             SELECT {BRIEF_COLUMNS}, substr(n.body, 1, ?2), n.basis, n.verified_at, s.via,
                    count(*) OVER ()
             FROM nearest s JOIN notes n ON n.id = s.id
             WHERE n.status <> 'dropped' AND n.id NOT IN (SELECT id FROM up)
             ORDER BY s.distance, n.id LIMIT ?3",
            lineage = lineage()
        ))?
        .query_map(params![task, (max_chars + 1) as i64, SECTION_ROWS], |row| {
            total = row.get(12)?;
            Ok(SharedNote {
                excerpt: Excerpt::new(brief_from_row(row)?, &row.get::<_, String>(8)?, max_chars),
                basis: row.get(9)?,
                verified_at: row.get(10)?,
                superseded_by: Vec::new(),
                contradicted_by: Vec::new(),
                via: row.get(11)?,
            })
        })?
        .collect::<rusqlite::Result<_>>()?;

    let ids: Vec<NoteId> = notes.iter().map(|shared| shared.excerpt.note.id).collect();
    let ids = serde_json::to_string(&ids).map_err(|error| MemoryError::Internal(error.into()))?;
    for (target, challenger, kind) in recall::challenges(conn, &ids)? {
        let Some(shared) = notes
            .iter_mut()
            .find(|shared| shared.excerpt.note.id == target)
        else {
            continue;
        };
        match kind {
            EdgeKind::Supersedes => shared.superseded_by.push(challenger),
            _ => shared.contradicted_by.push(challenger),
        }
    }
    Ok((total, notes))
}

// The notes above the task, nearest first: what the work is part of.
fn ancestors(conn: &Connection, task: NoteId, max_chars: usize) -> Result<Vec<Excerpt>> {
    Ok(conn
        .prepare(&format!(
            "{lineage}
             SELECT {BRIEF_COLUMNS}, substr(n.body, 1, ?2)
             FROM up JOIN notes n ON n.id = up.id
             WHERE up.distance > 0
             ORDER BY up.distance LIMIT ?3",
            lineage = lineage()
        ))?
        .query_map(params![task, (max_chars + 1) as i64, SECTION_ROWS], |row| {
            Ok(Excerpt::new(
                brief_from_row(row)?,
                &row.get::<_, String>(8)?,
                max_chars,
            ))
        })?
        .collect::<rusqlite::Result<_>>()?)
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Query {
    pub task: NoteId,
    pub children_limit: Option<usize>,
    pub children_cursor: Option<NoteId>,
    // The tasks above are background the agent was not necessarily given.
    #[serde(default)]
    pub include_ancestors: bool,
}

// No character limit: the area's summary, brief and checkpoint come whole.
// Only lists are bounded, by rows, and what they leave out is counted.
pub fn resume(conn: &Connection, query: &Query) -> Result<Value> {
    let task = query.task;
    // One transaction, so the summary, its freshness and the page agree.
    let tx = conn.unchecked_transaction()?;
    let mut area = summaries::read_in(
        &tx,
        &summaries::Query {
            task,
            children_limit: query.children_limit,
            children_cursor: query.children_cursor,
            children_offset: None,
            include_child_summaries: false,
        },
        None,
    )?;

    // The brief the parent wrote stays readable after the summary is rewritten.
    area["task"]["brief"] = json!(load(&tx, task)?.body);

    let checkpoint = tx
        .query_row(
            &format!("SELECT {CHECKPOINT_COLUMNS} FROM checkpoints WHERE task_id = ?1 ORDER BY id DESC LIMIT 1"),
            [task],
            checkpoint_from_row,
        )
        .optional()?;
    let since = checkpoint.as_ref().map_or(0, |saved| saved.journal_id);
    let checkpoint = match checkpoint {
        None => Value::Null,
        Some(saved) => {
            serde_json::to_value(&saved).map_err(|error| MemoryError::Internal(error.into()))?
        }
    };

    let (handed_down, shared) = shared(&tx, task, WHOLE_TEXT)?;
    let mut context = json!({
        "shared": shared,
        "shared_omitted": handed_down - shared.len() as i64,
    });
    if query.include_ancestors {
        context["ancestors"] = json!(ancestors(&tx, task, WHOLE_TEXT)?);
    }
    let (changed, _) = journal::task_changes(&tx, task, since, 0)?;
    // What is left to tick off says more about the state than any status.
    area["checklist"] = practices::open_items(&tx, task)?;
    tx.commit()?;

    area["checkpoint"] = checkpoint;
    area["context"] = context;
    area["changes_since_checkpoint"] = json!(changed);
    Ok(area)
}
