// A scoped resume starts at one task: its whole summary and one page of
// immediate child descriptors. Another node needs another explicit request.

use rusqlite::Connection;
use serde::Deserialize;
use serde_json::Value;

use super::model::{MemoryError, NoteId};
use super::summaries;

type Result<T> = std::result::Result<T, MemoryError>;

pub(super) const SUBTREE: &str = "\
    WITH RECURSIVE sub (id) AS (
        SELECT ?1
        UNION
        SELECT c.id FROM notes c JOIN sub ON c.parent_id = sub.id
    )
    SELECT id FROM sub";

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Query {
    pub task: NoteId,
    pub children_limit: Option<usize>,
    pub children_cursor: Option<NoteId>,
}

pub fn resume(conn: &Connection, query: &Query) -> Result<Value> {
    summaries::read(
        conn,
        &summaries::Query {
            task: query.task,
            children_limit: query.children_limit,
            children_cursor: query.children_cursor,
            children_offset: None,
        },
    )
}
