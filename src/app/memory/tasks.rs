// Resuming one task (a note and its subtree): last checkpoint, progress,
// blockers, failures, changes since. Sections fill a character budget in
// order of importance; what did not fit is counted. A section reads at most
// 100 rows.

use rusqlite::{Connection, OptionalExtension, params};
use serde::Serialize;
use serde_json::{Map, Value, json};

use super::journal::{self, HistoryEntry};
use super::model::{MemoryError, NoteBrief, NoteId, invalid, now};
use super::notes::{BRIEF_COLUMNS, CHECKPOINT_COLUMNS, brief_from_row, checkpoint_from_row, load};

pub const DEFAULT_BUDGET_CHARS: usize = 8_000;
pub const MIN_BUDGET_CHARS: usize = 1_000;
pub const MAX_BUDGET_CHARS: usize = 60_000;
const SECTION_ROWS: i64 = 100;
const PREVIEW_CHARS: usize = 400;
const TRUNCATION_MARK: &str = " ... [truncated]";

type Result<T> = std::result::Result<T, MemoryError>;

pub(super) const SUBTREE: &str = "\
    WITH RECURSIVE sub (id) AS (
        SELECT ?1
        UNION
        SELECT c.id FROM notes c JOIN sub ON c.parent_id = sub.id
    )
    SELECT id FROM sub";

#[derive(Serialize)]
struct WithPreview {
    #[serde(flatten)]
    note: NoteBrief,
    #[serde(skip_serializing_if = "String::is_empty")]
    preview: String,
}

#[derive(Serialize)]
struct BlockedStep {
    #[serde(flatten)]
    step: NoteBrief,
    waiting_on: Vec<NoteId>,
}

#[derive(Serialize)]
struct Claim {
    entity: i64,
    #[serde(rename = "type")]
    kind: String,
    key: String,
    claimed_by: Option<String>,
    claimed_until: String,
}

pub(super) fn truncated(text: &str, max_chars: usize) -> (String, bool) {
    if text.chars().count() <= max_chars {
        return (text.to_string(), false);
    }
    let kept: String = text.chars().take(max_chars).collect();
    (format!("{kept}{TRUNCATION_MARK}"), true)
}

fn briefs(conn: &Connection, task: NoteId, filter: &str) -> Result<Vec<NoteBrief>> {
    Ok(conn
        .prepare(&format!(
            "SELECT {BRIEF_COLUMNS} FROM notes n
             WHERE n.id IN ({SUBTREE}) AND n.id <> ?1 AND {filter}
             ORDER BY n.updated_at DESC, n.id DESC LIMIT ?2"
        ))?
        .query_map(params![task, SECTION_ROWS], brief_from_row)?
        .collect::<rusqlite::Result<_>>()?)
}

fn with_previews(conn: &Connection, task: NoteId, filter: &str) -> Result<Vec<WithPreview>> {
    Ok(conn
        .prepare(&format!(
            "SELECT {BRIEF_COLUMNS}, substr(n.body, 1, ?3) FROM notes n
             WHERE n.id IN ({SUBTREE}) AND n.id <> ?1 AND {filter}
             ORDER BY n.updated_at DESC, n.id DESC LIMIT ?2"
        ))?
        .query_map(params![task, SECTION_ROWS, PREVIEW_CHARS as i64], |row| {
            Ok(WithPreview {
                note: brief_from_row(row)?,
                preview: row.get(8)?,
            })
        })?
        .collect::<rusqlite::Result<_>>()?)
}

// Unfinished steps that depend on a note which is not done yet.
fn blocked_steps(conn: &Connection, task: NoteId) -> Result<Vec<BlockedStep>> {
    let rows: Vec<(NoteBrief, String)> = conn
        .prepare(&format!(
            "SELECT {BRIEF_COLUMNS}, json_group_array(d.id)
             FROM notes n
             JOIN edges e ON e.src = n.id AND e.kind = 'depends_on'
             JOIN notes d ON d.id = e.dst AND d.status NOT IN ('done', 'dropped')
             WHERE n.id IN ({SUBTREE}) AND n.kind IN ('goal', 'step')
               AND n.status IN ('open', 'active')
             GROUP BY n.id ORDER BY n.id LIMIT ?2"
        ))?
        .query_map(params![task, SECTION_ROWS], |row| {
            Ok((brief_from_row(row)?, row.get(8)?))
        })?
        .collect::<rusqlite::Result<_>>()?;
    Ok(rows
        .into_iter()
        .map(|(step, waiting_on)| BlockedStep {
            step,
            waiting_on: serde_json::from_str(&waiting_on).unwrap_or_default(),
        })
        .collect())
}

// Entities the task's notes are about that someone holds right now.
fn claims(conn: &Connection, task: NoteId) -> Result<Vec<Claim>> {
    Ok(conn
        .prepare(&format!(
            "SELECT e.id, e.type, e.key, e.claimed_by, e.claim_expires_at FROM entities e
             WHERE e.claim_id IS NOT NULL AND e.claim_expires_at > ?3
               AND e.id IN (SELECT entity_id FROM notes WHERE id IN ({SUBTREE}))
             ORDER BY e.id LIMIT ?2"
        ))?
        .query_map(params![task, SECTION_ROWS, now()], |row| {
            Ok(Claim {
                entity: row.get(0)?,
                kind: row.get(1)?,
                key: row.get(2)?,
                claimed_by: row.get(3)?,
                claimed_until: row.get(4)?,
            })
        })?
        .collect::<rusqlite::Result<_>>()?)
}

fn progress(conn: &Connection, task: NoteId) -> Result<Value> {
    let counts: Vec<(String, i64)> = conn
        .prepare(&format!(
            "SELECT status, count(*) FROM notes
             WHERE id IN ({SUBTREE}) AND id <> ?1 AND kind = 'step' GROUP BY status"
        ))?
        .query_map([task], |row| Ok((row.get(0)?, row.get(1)?)))?
        .collect::<rusqlite::Result<_>>()?;
    Ok(Value::Object(
        counts
            .into_iter()
            .map(|(status, count)| (status, json!(count)))
            .collect(),
    ))
}

// Fills sections in the order given, until the budget is spent.
struct Budget {
    limit: usize,
    used: usize,
    omitted: Map<String, Value>,
}

impl Budget {
    fn cost(value: &Value) -> usize {
        value.to_string().chars().count()
    }

    fn spend(&mut self, value: &Value) {
        self.used += Self::cost(value);
    }

    fn left(&self) -> usize {
        self.limit.saturating_sub(self.used)
    }

    fn fit<T: Serialize>(&mut self, section: &str, items: Vec<T>) -> Result<Value> {
        let mut kept = Vec::new();
        let total = items.len();
        for item in items {
            let value =
                serde_json::to_value(item).map_err(|error| MemoryError::Internal(error.into()))?;
            let cost = Self::cost(&value);
            if cost > self.left() {
                break;
            }
            self.used += cost;
            kept.push(value);
        }
        if kept.len() < total {
            self.omitted
                .insert(section.to_string(), json!(total - kept.len()));
        }
        Ok(Value::from(kept))
    }
}

pub fn resume(conn: &Connection, task: NoteId, budget_chars: Option<usize>) -> Result<Value> {
    let limit = budget_chars.unwrap_or(DEFAULT_BUDGET_CHARS);
    if !(MIN_BUDGET_CHARS..=MAX_BUDGET_CHARS).contains(&limit) {
        return Err(invalid(format!(
            "budget_chars must be between {MIN_BUDGET_CHARS} and {MAX_BUDGET_CHARS}"
        )));
    }
    let note = load(conn, task)?;
    let mut budget = Budget {
        limit,
        used: 0,
        omitted: Map::new(),
    };

    let checkpoint = conn
        .query_row(
            &format!(
                "SELECT {CHECKPOINT_COLUMNS} FROM checkpoints WHERE task_id = ?1
                 ORDER BY id DESC LIMIT 1"
            ),
            [task],
            checkpoint_from_row,
        )
        .optional()?;
    let since = checkpoint.as_ref().map_or(0, |saved| saved.journal_id);

    // The checkpoint is what the last session wanted read first: up to half the budget.
    let checkpoint = match checkpoint {
        None => Value::Null,
        Some(saved) => {
            let (summary, cut) = truncated(&saved.summary, limit / 2);
            let mut value = serde_json::to_value(&saved)
                .map_err(|error| MemoryError::Internal(error.into()))?;
            value["summary"] = json!(summary);
            if cut {
                value["summary_truncated"] = json!(true);
            }
            value
        }
    };
    budget.spend(&checkpoint);

    let (body, body_cut) = truncated(&note.body, limit / 8);
    let mut task_value = json!({
        "id": note.id,
        "project": note.project,
        "kind": note.kind,
        "status": note.status,
        "title": note.title,
        "body": body,
        "parent": note.parent,
        "revision": note.revision,
        "updated_at": note.updated_at,
    });
    if body_cut {
        task_value["body_truncated"] = json!(true);
    }
    budget.spend(&task_value);
    let progress = progress(conn, task)?;
    budget.spend(&progress);

    let open_questions = budget.fit(
        "open_questions",
        briefs(conn, task, "n.kind = 'question' AND n.status = 'open'")?,
    )?;
    let blocked = budget.fit("blocked_steps", blocked_steps(conn, task)?)?;
    let claims = budget.fit("claims", claims(conn, task)?)?;
    let active_steps = budget.fit(
        "active_steps",
        briefs(conn, task, "n.kind = 'step' AND n.status = 'active'")?,
    )?;
    let failed_attempts = budget.fit(
        "failed_attempts",
        with_previews(
            conn,
            task,
            "n.status = 'failed' AND n.kind IN ('attempt', 'step')",
        )?,
    )?;
    let (changed, entries): (i64, Vec<HistoryEntry>) =
        journal::task_changes(conn, task, since, SECTION_ROWS as usize)?;
    let listed = entries.len() as i64;
    let entries = budget.fit("changes_since_checkpoint", entries)?;
    // Changes beyond the listing limit are omitted too.
    if changed > listed {
        let dropped = budget
            .omitted
            .get("changes_since_checkpoint")
            .and_then(Value::as_i64)
            .unwrap_or(0);
        budget.omitted.insert(
            "changes_since_checkpoint".into(),
            json!(dropped + changed - listed),
        );
    }
    let next_steps = budget.fit(
        "next_steps",
        briefs(conn, task, "n.kind = 'step' AND n.status = 'open'")?,
    )?;
    let decisions = budget.fit(
        "decisions",
        with_previews(conn, task, "n.kind = 'decision' AND n.status = 'active'")?,
    )?;

    Ok(json!({
        "task": task_value,
        "checkpoint": checkpoint,
        "progress": progress,
        "blockers": {
            "open_questions": open_questions,
            "blocked_steps": blocked,
            "claims": claims,
        },
        "active_steps": active_steps,
        "failed_attempts": failed_attempts,
        "changes_since_checkpoint": { "count": changed, "entries": entries },
        "next_steps": next_steps,
        "decisions": decisions,
        "budget": { "limit": limit, "used": budget.used, "omitted": budget.omitted },
    }))
}
