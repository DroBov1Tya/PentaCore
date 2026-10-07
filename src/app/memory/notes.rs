use rusqlite::{Connection, OptionalExtension, Row, TransactionBehavior, params};

use super::db::{Db, Tree};

use super::model::{
    Checkpoint, CreatedNote, Edge, EdgeKind, EntityId, FindQuery, FindResult, Graph, GraphNode,
    Hit, Limit, LinkedNote, MAX_BODY_BYTES, MemoryError, NewCheckpoint, NewNote, Note, NoteBrief,
    NoteDetail, NoteId, NoteKind, NotePatch, ProjectName, Scope, Snapshot, invalid, names, now,
    tag_strings,
};

const DEFAULT_FIND_LIMIT: usize = 10;
const MAX_GRAPH_NODES: usize = 500;
const MAX_GRAPH_EDGES: usize = 1000;
const MAX_CHILDREN_LISTED: i64 = 200;
const SNAPSHOT_SECTION_LIMIT: i64 = 30;
const SNAPSHOT_RECENT_LIMIT: i64 = 20;
const SIMILAR_LIMIT: i64 = 3;
const CHECKPOINTS_KEPT: i64 = 20;

const NOTE_COLUMNS: &str = "id, project, kind, status, title, body, tags, parent_id, created_at, updated_at, entity_id, author";
const BRIEF_COLUMNS: &str = "n.id, n.project, n.kind, n.status, n.title, n.parent_id, n.updated_at";

type Result<T> = std::result::Result<T, MemoryError>;

#[derive(Clone)]
pub struct NoteStore {
    db: Db,
}

impl NoteStore {
    pub fn new(db: Db) -> Self {
        Self { db }
    }

    #[cfg(test)]
    pub fn in_memory() -> Self {
        Self::new(Db::in_memory())
    }

    pub async fn create(
        &self,
        input: NewNote,
        default_project: ProjectName,
    ) -> Result<CreatedNote> {
        self.db
            .run(move |conn| create(conn, input, default_project))
            .await
    }

    pub async fn update(&self, patch: NotePatch) -> Result<Note> {
        self.db.run(move |conn| update(conn, patch)).await
    }

    pub async fn get(&self, id: NoteId) -> Result<NoteDetail> {
        self.db.run(move |conn| detail(conn, id)).await
    }

    pub async fn link(&self, edge: Edge) -> Result<Edge> {
        self.db
            .run(move |conn| {
                let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
                insert_edge(&tx, &edge)?;
                tx.commit()?;
                Ok(edge)
            })
            .await
    }

    pub async fn unlink(&self, edge: Edge) -> Result<bool> {
        self.db
            .run(move |conn| {
                let removed = conn.execute(
                    "DELETE FROM edges WHERE src = ?1 AND dst = ?2 AND kind = ?3",
                    params![edge.src, edge.dst, edge.kind],
                )?;
                Ok(removed > 0)
            })
            .await
    }

    pub async fn find(&self, query: FindQuery, default_scope: Scope) -> Result<FindResult> {
        self.db
            .run(move |conn| find(conn, query, default_scope))
            .await
    }

    pub async fn graph(&self, root: NoteId, max_depth: u8) -> Result<Graph> {
        self.db.run(move |conn| graph(conn, root, max_depth)).await
    }

    pub async fn snapshot(&self, scope: Scope) -> Result<Snapshot> {
        self.db.run(move |conn| snapshot(conn, &scope)).await
    }

    pub async fn checkpoint(
        &self,
        input: NewCheckpoint,
        default_project: ProjectName,
    ) -> Result<Checkpoint> {
        self.db
            .run(move |conn| save_checkpoint(conn, input, default_project))
            .await
    }

    pub async fn forget(&self, id: NoteId, recursive: bool) -> Result<i64> {
        self.db.run(move |conn| forget(conn, id, recursive)).await
    }
}

fn note_from_row(row: &Row<'_>) -> rusqlite::Result<Note> {
    let tags: String = row.get(6)?;
    Ok(Note {
        id: row.get(0)?,
        project: row.get(1)?,
        kind: row.get(2)?,
        status: row.get(3)?,
        title: row.get(4)?,
        body: row.get(5)?,
        tags: tags.split_whitespace().map(String::from).collect(),
        parent: row.get(7)?,
        created_at: row.get(8)?,
        updated_at: row.get(9)?,
        entity: row.get(10)?,
        author: row.get(11)?,
    })
}

fn brief_from_row(row: &Row<'_>) -> rusqlite::Result<NoteBrief> {
    Ok(NoteBrief {
        id: row.get(0)?,
        project: row.get(1)?,
        kind: row.get(2)?,
        status: row.get(3)?,
        title: row.get(4)?,
        parent: row.get(5)?,
        updated_at: row.get(6)?,
    })
}

fn not_found(id: NoteId) -> MemoryError {
    MemoryError::NotFound(format!("note {id}"))
}

fn load(conn: &Connection, id: NoteId) -> Result<Note> {
    conn.query_row(
        &format!("SELECT {NOTE_COLUMNS} FROM notes WHERE id = ?1"),
        [id],
        note_from_row,
    )
    .optional()?
    .ok_or_else(|| not_found(id))
}

fn ensure_exists(conn: &Connection, id: NoteId) -> Result<()> {
    let exists: bool = conn.query_row(
        "SELECT EXISTS (SELECT 1 FROM notes WHERE id = ?1)",
        [id],
        |row| row.get(0),
    )?;
    if exists { Ok(()) } else { Err(not_found(id)) }
}

fn create(
    conn: &mut Connection,
    input: NewNote,
    default_project: ProjectName,
) -> Result<CreatedNote> {
    let status = match input.status.or(input.kind.default_status()) {
        Some(status) => status,
        None => {
            return Err(invalid(format!(
                "a {} needs an explicit status: {}",
                input.kind,
                names(input.kind.allowed_statuses())
            )));
        }
    };
    input.kind.check_status(status)?;

    let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let project = project_for_new_note(&tx, &input, default_project)?;
    if let Some(entity) = input.entity {
        check_entity(&tx, entity, &project)?;
    }
    let now = now();
    tx.execute(
        "INSERT INTO notes
             (project, kind, status, title, body, tags, parent_id, entity_id, author, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?10)",
        params![
            project,
            input.kind,
            status,
            input.title.as_str(),
            input.body.as_str(),
            tag_strings(&input.tags).join(" "),
            input.parent,
            input.entity,
            input.author.as_ref().map(|author| author.as_str()),
            now,
        ],
    )?;
    let id = tx.last_insert_rowid();
    for link in input.links.as_slice() {
        insert_edge(
            &tx,
            &Edge {
                src: id,
                dst: link.to,
                kind: link.kind,
            },
        )?;
    }
    let note = load(&tx, id)?;
    let similar = similar_notes(&tx, &note)?;
    tx.commit()?;
    Ok(CreatedNote { note, similar })
}

fn check_entity(conn: &Connection, entity: EntityId, project: &str) -> Result<()> {
    let entity_project: Option<String> = conn
        .query_row(
            "SELECT project FROM entities WHERE id = ?1",
            [entity],
            |row| row.get(0),
        )
        .optional()?;
    match entity_project {
        None => Err(MemoryError::NotFound(format!("entity {entity}"))),
        Some(other) if other != project => Err(invalid(format!(
            "entity {entity} belongs to project '{other}', the note to '{project}'"
        ))),
        Some(_) => Ok(()),
    }
}

fn similar_notes(conn: &Connection, note: &Note) -> Result<Vec<NoteBrief>> {
    let Some(expression) = fts_expression(&note.title, Match::All) else {
        return Ok(Vec::new());
    };
    let similar = conn
        .prepare(&format!(
            "SELECT {BRIEF_COLUMNS}
             FROM notes_fts
             JOIN notes n ON n.id = notes_fts.rowid
             WHERE notes_fts MATCH ?1 AND n.project = ?2 AND n.id <> ?3
             ORDER BY bm25(notes_fts, 4.0, 1.0, 2.0), n.id DESC
             LIMIT ?4"
        ))?
        .query_map(
            params![expression, note.project, note.id, SIMILAR_LIMIT],
            brief_from_row,
        )?
        .collect::<rusqlite::Result<_>>()?;
    Ok(similar)
}

// A child takes its parent's project, so a tree never spans projects.
fn project_for_new_note(
    conn: &Connection,
    input: &NewNote,
    default_project: ProjectName,
) -> Result<String> {
    let Some(parent) = input.parent else {
        return Ok(input.project.clone().unwrap_or(default_project).into());
    };
    if input.kind == NoteKind::Goal {
        return Err(invalid(
            "a goal is a root and cannot have a parent; use a step",
        ));
    }
    let parent_project = load(conn, parent)?.project;
    Tree::Notes.check_depth(conn, parent, None)?;
    match &input.project {
        Some(project) if project.as_str() != parent_project => Err(invalid(format!(
            "parent {parent} belongs to project '{parent_project}', not '{}'",
            project.as_str()
        ))),
        _ => Ok(parent_project),
    }
}

fn insert_edge(conn: &Connection, edge: &Edge) -> Result<()> {
    if edge.src == edge.dst {
        return Err(invalid("a note cannot link to itself"));
    }
    ensure_exists(conn, edge.src)?;
    ensure_exists(conn, edge.dst)?;
    if edge.kind == EdgeKind::DependsOn && depends_on(conn, edge.dst, edge.src)? {
        return Err(MemoryError::Conflict(format!(
            "note {} already depends on note {}; this link would close a cycle",
            edge.dst, edge.src
        )));
    }
    conn.execute(
        "INSERT OR IGNORE INTO edges (src, dst, kind, created_at) VALUES (?1, ?2, ?3, ?4)",
        params![edge.src, edge.dst, edge.kind, now()],
    )?;
    Ok(())
}

fn depends_on(conn: &Connection, from: NoteId, to: NoteId) -> Result<bool> {
    Ok(conn.query_row(
        "WITH RECURSIVE reach (id) AS (
             SELECT ?1
             UNION
             SELECT e.dst FROM edges e JOIN reach ON e.src = reach.id WHERE e.kind = 'depends_on'
         )
         SELECT EXISTS (SELECT 1 FROM reach WHERE id = ?2)",
        [from, to],
        |row| row.get(0),
    )?)
}

fn update(conn: &mut Connection, patch: NotePatch) -> Result<Note> {
    let NotePatch {
        id,
        title,
        body,
        status,
        tags,
        parent,
        append,
        entity,
    } = patch;
    let fields_given = [
        title.is_some(),
        body.is_some(),
        status.is_some(),
        tags.is_some(),
        parent.is_some(),
        append.is_some(),
        entity.is_some(),
    ];
    if !fields_given.contains(&true) {
        return Err(invalid(
            "nothing to update: give at least one field besides id",
        ));
    }
    if body.is_some() && append.is_some() {
        return Err(invalid("give either body or append, not both"));
    }

    let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let current = load(&tx, id)?;
    if let Some(status) = status {
        current.kind.check_status(status)?;
    }
    if let Some(parent) = parent {
        check_new_parent(&tx, &current, parent)?;
    }
    if let Some(entity) = entity {
        check_entity(&tx, entity, &current.project)?;
    }
    let body = match (body, append) {
        (Some(body), _) => body.into(),
        (None, Some(extra)) => appended(&current.body, extra.as_str())?,
        (None, None) => current.body,
    };

    tx.execute(
        "UPDATE notes
         SET title = ?2, body = ?3, tags = ?4, status = ?5, parent_id = ?6, entity_id = ?7,
             updated_at = ?8
         WHERE id = ?1",
        params![
            id,
            title.map_or(current.title, String::from),
            body,
            tags.map_or(current.tags.join(" "), |tags| tag_strings(&tags).join(" ")),
            status.unwrap_or(current.status),
            parent.or(current.parent),
            entity.or(current.entity),
            now(),
        ],
    )?;
    let note = load(&tx, id)?;
    tx.commit()?;
    Ok(note)
}

fn appended(body: &str, extra: &str) -> Result<String> {
    if extra.is_empty() {
        return Err(invalid("append must not be empty"));
    }
    let joined = if body.is_empty() {
        extra.to_string()
    } else {
        format!("{body}\n{extra}")
    };
    if joined.len() > MAX_BODY_BYTES {
        return Err(invalid(format!(
            "the body would grow beyond {MAX_BODY_BYTES} bytes"
        )));
    }
    Ok(joined)
}

fn check_new_parent(conn: &Connection, note: &Note, parent: NoteId) -> Result<()> {
    if note.kind == NoteKind::Goal {
        return Err(invalid("a goal is a root and cannot have a parent"));
    }
    let parent_project = load(conn, parent)?.project;
    if parent_project != note.project {
        return Err(invalid(format!(
            "parent {parent} belongs to project '{parent_project}', the note to '{}'",
            note.project
        )));
    }
    if Tree::Notes.contains(conn, note.id, parent)? {
        return Err(MemoryError::Conflict(format!(
            "note {parent} is note {} or one of its descendants; this move would close a cycle",
            note.id
        )));
    }
    Tree::Notes.check_depth(conn, parent, Some(note.id))
}

fn detail(conn: &Connection, id: NoteId) -> Result<NoteDetail> {
    let note = load(conn, id)?;

    let children = conn
        .prepare(&format!(
            "SELECT {BRIEF_COLUMNS} FROM notes n WHERE n.parent_id = ?1 ORDER BY n.id LIMIT ?2"
        ))?
        .query_map(params![id, MAX_CHILDREN_LISTED], brief_from_row)?
        .collect::<rusqlite::Result<_>>()?;

    let links = conn
        .prepare(&format!(
            "SELECT e.src, e.dst, e.kind, {BRIEF_COLUMNS}
             FROM edges e
             JOIN notes n ON n.id = CASE WHEN e.src = ?1 THEN e.dst ELSE e.src END
             WHERE e.src = ?1 OR e.dst = ?1
             ORDER BY e.created_at, e.src, e.dst
             LIMIT ?2"
        ))?
        .query_map(params![id, MAX_CHILDREN_LISTED], |row| {
            Ok(LinkedNote {
                edge: Edge {
                    src: row.get(0)?,
                    dst: row.get(1)?,
                    kind: row.get(2)?,
                },
                peer: NoteBrief {
                    id: row.get(3)?,
                    project: row.get(4)?,
                    kind: row.get(5)?,
                    status: row.get(6)?,
                    title: row.get(7)?,
                    parent: row.get(8)?,
                    updated_at: row.get(9)?,
                },
            })
        })?
        .collect::<rusqlite::Result<_>>()?;

    Ok(NoteDetail {
        note,
        children,
        links,
    })
}

#[derive(Clone, Copy)]
enum Match {
    All,
    AllPrefix,
    AnyPrefix,
}

impl Match {
    const LADDER: [Self; 3] = [Self::All, Self::AllPrefix, Self::AnyPrefix];

    fn label(self) -> &'static str {
        match self {
            Self::All => "all",
            Self::AllPrefix => "prefix",
            Self::AnyPrefix => "any",
        }
    }
}

// Quotes every word, so FTS5 operators in user text stay plain text.
fn fts_expression(text: &str, mode: Match) -> Option<String> {
    let (prefix_mark, joiner) = match mode {
        Match::All => ("", " AND "),
        Match::AllPrefix => ("*", " AND "),
        Match::AnyPrefix => ("*", " OR "),
    };
    let phrases: Vec<String> = text
        .split_whitespace()
        .filter(|term| term.chars().any(char::is_alphanumeric))
        .map(|term| format!("\"{}\"{prefix_mark}", term.replace('"', "\"\"")))
        .collect();
    if phrases.is_empty() {
        None
    } else {
        Some(phrases.join(joiner))
    }
}

fn find(conn: &Connection, query: FindQuery, default_scope: Scope) -> Result<FindResult> {
    for mode in Match::LADDER {
        let expression = fts_expression(query.query.as_str(), mode)
            .ok_or_else(|| invalid("query has no letters or digits to search for"))?;
        let hits = search(conn, &expression, &query, &default_scope)?;
        if !hits.is_empty() {
            return Ok(FindResult {
                matched: mode.label(),
                hits,
            });
        }
    }
    Ok(FindResult {
        matched: Match::All.label(),
        hits: Vec::new(),
    })
}

fn search(
    conn: &Connection,
    expression: &str,
    query: &FindQuery,
    default: &Scope,
) -> Result<Vec<Hit>> {
    let scope = query.project.as_ref().unwrap_or(default);
    let limit = Limit::or(query.limit, DEFAULT_FIND_LIMIT) as i64;
    let hits = conn
        .prepare(&format!(
            "SELECT {BRIEF_COLUMNS}, snippet(notes_fts, -1, '[', ']', ' ... ', 24)
             FROM notes_fts
             JOIN notes n ON n.id = notes_fts.rowid
             WHERE notes_fts MATCH ?1
               AND (?2 IS NULL OR n.project = ?2)
               AND (?3 IS NULL OR n.kind = ?3)
               AND (?4 IS NULL OR n.status = ?4)
               AND (?5 IS NULL OR n.id IN (
                   WITH RECURSIVE sub (id) AS (
                       SELECT ?5
                       UNION
                       SELECT c.id FROM notes c JOIN sub ON c.parent_id = sub.id
                   )
                   SELECT id FROM sub))
               AND (?7 IS NULL OR n.entity_id = ?7)
             ORDER BY bm25(notes_fts, 4.0, 1.0, 2.0), n.id DESC
             LIMIT ?6"
        ))?
        .query_map(
            params![
                expression,
                scope.project(),
                query.kind,
                query.status,
                query.under,
                limit,
                query.entity
            ],
            |row| {
                Ok(Hit {
                    note: brief_from_row(row)?,
                    snippet: row.get(7)?,
                })
            },
        )?
        .collect::<rusqlite::Result<_>>()?;
    Ok(hits)
}

fn graph(conn: &Connection, root: NoteId, max_depth: u8) -> Result<Graph> {
    ensure_exists(conn, root)?;

    // Zero padded path sorts rows in depth first order.
    let mut nodes: Vec<GraphNode> = conn
        .prepare(
            "WITH RECURSIVE tree (id, depth, path) AS (
                 SELECT id, 0, printf('%019d', id) FROM notes WHERE id = ?1
                 UNION ALL
                 SELECT c.id, tree.depth + 1, tree.path || printf('/%019d', c.id)
                 FROM notes c JOIN tree ON c.parent_id = tree.id
                 WHERE tree.depth < ?2
             )
             SELECT n.id, n.parent_id, tree.depth, n.kind, n.status, n.title
             FROM tree JOIN notes n ON n.id = tree.id
             ORDER BY tree.path
             LIMIT ?3",
        )?
        .query_map(
            params![root, max_depth, MAX_GRAPH_NODES as i64 + 1],
            |row| {
                Ok(GraphNode {
                    id: row.get(0)?,
                    parent: row.get(1)?,
                    depth: row.get(2)?,
                    kind: row.get(3)?,
                    status: row.get(4)?,
                    title: row.get(5)?,
                })
            },
        )?
        .collect::<rusqlite::Result<_>>()?;

    let mut edges: Vec<Edge> = conn
        .prepare(
            "WITH RECURSIVE sub (id) AS (
                 SELECT ?1
                 UNION
                 SELECT c.id FROM notes c JOIN sub ON c.parent_id = sub.id
             )
             SELECT src, dst, kind FROM edges
             WHERE src IN (SELECT id FROM sub) OR dst IN (SELECT id FROM sub)
             ORDER BY src, dst
             LIMIT ?2",
        )?
        .query_map(params![root, MAX_GRAPH_EDGES as i64 + 1], |row| {
            Ok(Edge {
                src: row.get(0)?,
                dst: row.get(1)?,
                kind: row.get(2)?,
            })
        })?
        .collect::<rusqlite::Result<_>>()?;

    let truncated = nodes.len() > MAX_GRAPH_NODES || edges.len() > MAX_GRAPH_EDGES;
    nodes.truncate(MAX_GRAPH_NODES);
    edges.truncate(MAX_GRAPH_EDGES);
    Ok(Graph {
        nodes,
        edges,
        truncated,
    })
}

fn snapshot(conn: &Connection, scope: &Scope) -> Result<Snapshot> {
    let project = scope.project();
    let section = |filter: &str, limit: i64| -> Result<Vec<NoteBrief>> {
        Ok(conn
            .prepare(&format!(
                "SELECT {BRIEF_COLUMNS} FROM notes n
                 WHERE (?1 IS NULL OR n.project = ?1) AND {filter}
                 ORDER BY n.updated_at DESC, n.id DESC
                 LIMIT ?2"
            ))?
            .query_map(params![project, limit], brief_from_row)?
            .collect::<rusqlite::Result<_>>()?)
    };

    Ok(Snapshot {
        goals: section(
            "n.kind = 'goal' AND n.status IN ('open', 'active')",
            SNAPSHOT_SECTION_LIMIT,
        )?,
        active_steps: section(
            "n.kind = 'step' AND n.status = 'active'",
            SNAPSHOT_SECTION_LIMIT,
        )?,
        open_questions: section(
            "n.kind = 'question' AND n.status = 'open'",
            SNAPSHOT_SECTION_LIMIT,
        )?,
        recent: section("TRUE", SNAPSHOT_RECENT_LIMIT)?,
        total_notes: conn.query_row(
            "SELECT count(*) FROM notes WHERE (?1 IS NULL OR project = ?1)",
            [project],
            |row| row.get(0),
        )?,
        checkpoint: conn
            .query_row(
                &format!(
                    "SELECT {CHECKPOINT_COLUMNS} FROM checkpoints
                     WHERE (?1 IS NULL OR project = ?1)
                     ORDER BY id DESC LIMIT 1"
                ),
                [project],
                checkpoint_from_row,
            )
            .optional()?,
    })
}

const CHECKPOINT_COLUMNS: &str = "id, project, summary, author, created_at";

fn checkpoint_from_row(row: &Row<'_>) -> rusqlite::Result<Checkpoint> {
    Ok(Checkpoint {
        id: row.get(0)?,
        project: row.get(1)?,
        summary: row.get(2)?,
        author: row.get(3)?,
        created_at: row.get(4)?,
    })
}

// Keeps only the latest checkpoints of the project.
fn save_checkpoint(
    conn: &mut Connection,
    input: NewCheckpoint,
    default_project: ProjectName,
) -> Result<Checkpoint> {
    if input.summary.as_str().trim().is_empty() {
        return Err(invalid("summary must not be empty"));
    }
    let project: String = input.project.unwrap_or(default_project).into();

    let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
    tx.execute(
        "INSERT INTO checkpoints (project, summary, author, created_at) VALUES (?1, ?2, ?3, ?4)",
        params![
            project,
            input.summary.as_str(),
            input.author.as_ref().map(|author| author.as_str()),
            now()
        ],
    )?;
    let id = tx.last_insert_rowid();
    tx.execute(
        "DELETE FROM checkpoints
         WHERE project = ?1
           AND id NOT IN (SELECT id FROM checkpoints WHERE project = ?1 ORDER BY id DESC LIMIT ?2)",
        params![project, CHECKPOINTS_KEPT],
    )?;
    let checkpoint = tx.query_row(
        &format!("SELECT {CHECKPOINT_COLUMNS} FROM checkpoints WHERE id = ?1"),
        [id],
        checkpoint_from_row,
    )?;
    tx.commit()?;
    Ok(checkpoint)
}

fn forget(conn: &mut Connection, id: NoteId, recursive: bool) -> Result<i64> {
    let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
    ensure_exists(&tx, id)?;
    let subtree_size: i64 = tx.query_row(
        "WITH RECURSIVE sub (id) AS (
             SELECT ?1
             UNION
             SELECT c.id FROM notes c JOIN sub ON c.parent_id = sub.id
         )
         SELECT count(*) FROM sub",
        [id],
        |row| row.get(0),
    )?;
    if subtree_size > 1 && !recursive {
        return Err(MemoryError::Conflict(format!(
            "note {id} has {} notes beneath it; pass recursive=true to delete them too",
            subtree_size - 1
        )));
    }
    // Children and edges go through ON DELETE CASCADE.
    tx.execute("DELETE FROM notes WHERE id = ?1", [id])?;
    tx.commit()?;
    Ok(subtree_size)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::memory::model::Status;
    use serde_json::{Value, json};

    fn project() -> ProjectName {
        ProjectName::try_from("demo".to_string()).unwrap()
    }

    fn scope() -> Scope {
        Scope::Project(project())
    }

    async fn add(store: &NoteStore, note: Value) -> Note {
        try_add(store, note).await.unwrap()
    }

    async fn try_add(store: &NoteStore, note: Value) -> Result<Note> {
        try_create(store, note).await.map(|created| created.note)
    }

    async fn try_create(store: &NoteStore, note: Value) -> Result<CreatedNote> {
        store
            .create(serde_json::from_value(note).unwrap(), project())
            .await
    }

    async fn patch(store: &NoteStore, patch: Value) -> Result<Note> {
        store.update(serde_json::from_value(patch).unwrap()).await
    }

    async fn matched(store: &NoteStore, query: &str) -> (&'static str, Vec<NoteId>) {
        let query = serde_json::from_value(json!({ "query": query })).unwrap();
        let result = store.find(query, scope()).await.unwrap();
        (
            result.matched,
            result.hits.iter().map(|hit| hit.note.id).collect(),
        )
    }

    #[tokio::test]
    async fn find_matches_word_forms_by_prefix_after_exact_words() {
        let store = NoteStore::in_memory();
        let plural = add(&store, json!({"kind": "fact", "title": "Схемы таблиц"}))
            .await
            .id;
        let exact = add(&store, json!({"kind": "fact", "title": "Одна схем"}))
            .await
            .id;

        // An exact word beats a longer word that starts with it.
        assert_eq!(matched(&store, "схем").await, ("all", vec![exact]));
        assert_eq!(matched(&store, "схе табл").await, ("prefix", vec![plural]));
        assert_eq!(
            matched(&store, "табл отсутствует").await,
            ("any", vec![plural])
        );
        assert_eq!(matched(&store, "ничего").await, ("all", vec![]));
        assert_eq!(matched(&store, "отсутств*").await.1, Vec::<NoteId>::new());
    }

    #[tokio::test]
    async fn append_extends_the_body_within_the_limit() {
        let store = NoteStore::in_memory();
        let id = add(&store, json!({"kind": "fact", "title": "log"}))
            .await
            .id;

        assert_eq!(
            patch(&store, json!({"id": id, "append": "first"}))
                .await
                .unwrap()
                .body,
            "first"
        );
        assert_eq!(
            patch(&store, json!({"id": id, "append": "second"}))
                .await
                .unwrap()
                .body,
            "first\nsecond"
        );
        assert_eq!(find_ids(&store, json!({"query": "second"})).await, [id]);

        let both = patch(&store, json!({"id": id, "body": "x", "append": "y"})).await;
        assert!(matches!(both, Err(MemoryError::Invalid(_))));
        let empty = patch(&store, json!({"id": id, "append": ""})).await;
        assert!(matches!(empty, Err(MemoryError::Invalid(_))));
        let too_much = patch(
            &store,
            json!({"id": id, "append": "x".repeat(MAX_BODY_BYTES)}),
        )
        .await;
        assert!(matches!(too_much, Err(MemoryError::Invalid(_))));
        assert_eq!(store.get(id).await.unwrap().note.body, "first\nsecond");
    }

    #[tokio::test]
    async fn creating_a_note_reports_similar_ones_of_the_same_project() {
        let store = NoteStore::in_memory();
        let first = try_create(
            &store,
            json!({"kind": "fact", "title": "Linker flags", "author": "claude/a"}),
        )
        .await
        .unwrap();
        assert!(first.similar.is_empty());
        assert_eq!(first.note.author.as_deref(), Some("claude/a"));
        add(
            &store,
            json!({"kind": "fact", "title": "Linker flags", "project": "other"}),
        )
        .await;
        add(&store, json!({"kind": "fact", "title": "Compiler flags"})).await;

        let second = try_create(&store, json!({"kind": "decision", "title": "linker FLAGS"}))
            .await
            .unwrap();
        let similar: Vec<NoteId> = second.similar.iter().map(|note| note.id).collect();
        assert_eq!(similar, [first.note.id]);
        assert_eq!(
            serde_json::to_value(&second).unwrap()["similar"][0]["id"],
            first.note.id
        );
        assert!(
            serde_json::to_value(&first)
                .unwrap()
                .get("similar")
                .is_none()
        );
    }

    #[tokio::test]
    async fn checkpoints_keep_the_latest_per_project() {
        let store = NoteStore::in_memory();
        let save =
            |value: Value| store.checkpoint(serde_json::from_value(value).unwrap(), project());

        assert!(store.snapshot(scope()).await.unwrap().checkpoint.is_none());
        assert!(matches!(
            save(json!({"summary": "  "})).await,
            Err(MemoryError::Invalid(_))
        ));
        for round in 0..CHECKPOINTS_KEPT + 3 {
            save(json!({"summary": format!("round {round}"), "author": "claude"}))
                .await
                .unwrap();
        }
        save(json!({"summary": "elsewhere", "project": "other"}))
            .await
            .unwrap();

        let latest = store.snapshot(scope()).await.unwrap().checkpoint.unwrap();
        assert_eq!(latest.summary, format!("round {}", CHECKPOINTS_KEPT + 2));
        assert_eq!(
            store
                .snapshot(Scope::All)
                .await
                .unwrap()
                .checkpoint
                .unwrap()
                .summary,
            "elsewhere"
        );
        let kept: i64 = store
            .db
            .run(|conn| {
                Ok(conn.query_row(
                    "SELECT count(*) FROM checkpoints WHERE project = 'demo'",
                    [],
                    |r| r.get(0),
                )?)
            })
            .await
            .unwrap();
        assert_eq!(kept, CHECKPOINTS_KEPT);
    }

    async fn find_ids(store: &NoteStore, query: Value) -> Vec<NoteId> {
        let result = store
            .find(serde_json::from_value(query).unwrap(), scope())
            .await
            .unwrap();
        result.hits.iter().map(|hit| hit.note.id).collect()
    }

    fn edge(src: NoteId, dst: NoteId, kind: EdgeKind) -> Edge {
        Edge { src, dst, kind }
    }

    #[tokio::test]
    async fn created_note_gets_defaults() {
        let store = NoteStore::in_memory();
        let goal = add(
            &store,
            json!({"kind": "goal", "title": "Ship", "tags": ["a", "b"]}),
        )
        .await;
        assert_eq!(goal.project, "demo");
        assert_eq!(goal.status, Status::Open);
        assert_eq!(goal.tags, ["a", "b"]);
        assert_eq!(goal.parent, None);
    }

    #[tokio::test]
    async fn attempt_requires_a_valid_outcome() {
        let store = NoteStore::in_memory();
        let missing = try_add(&store, json!({"kind": "attempt", "title": "try"})).await;
        assert!(matches!(missing, Err(MemoryError::Invalid(_))));
        let wrong = try_add(
            &store,
            json!({"kind": "attempt", "title": "try", "status": "open"}),
        )
        .await;
        assert!(matches!(wrong, Err(MemoryError::Invalid(_))));
        let ok = add(
            &store,
            json!({"kind": "attempt", "title": "try", "status": "failed"}),
        )
        .await;
        assert_eq!(ok.status, Status::Failed);
    }

    #[tokio::test]
    async fn child_inherits_project_and_goal_stays_root() {
        let store = NoteStore::in_memory();
        let goal = add(
            &store,
            json!({"kind": "goal", "title": "G", "project": "other"}),
        )
        .await;
        let step = add(
            &store,
            json!({"kind": "step", "title": "S", "parent": goal.id}),
        )
        .await;
        assert_eq!(step.project, "other");

        let mismatch = try_add(
            &store,
            json!({"kind": "step", "title": "S", "parent": goal.id, "project": "demo"}),
        )
        .await;
        assert!(matches!(mismatch, Err(MemoryError::Invalid(_))));
        let nested_goal = try_add(
            &store,
            json!({"kind": "goal", "title": "G2", "parent": goal.id}),
        )
        .await;
        assert!(matches!(nested_goal, Err(MemoryError::Invalid(_))));
        let orphan = try_add(&store, json!({"kind": "step", "title": "S", "parent": 999})).await;
        assert!(matches!(orphan, Err(MemoryError::NotFound(_))));
    }

    #[tokio::test]
    async fn failed_link_rolls_back_the_note() {
        let store = NoteStore::in_memory();
        let result = try_add(
            &store,
            json!({"kind": "fact", "title": "unique-marker", "links": [{"kind": "supports", "to": 42}]}),
        )
        .await;
        assert!(matches!(result, Err(MemoryError::NotFound(_))));
        assert!(
            find_ids(&store, json!({"query": "unique-marker"}))
                .await
                .is_empty()
        );
    }

    #[tokio::test]
    async fn find_matches_concrete_text_and_respects_filters() {
        let store = NoteStore::in_memory();
        let goal = add(&store, json!({"kind": "goal", "title": "Fix build"})).await;
        let fact = add(
            &store,
            json!({"kind": "fact", "title": "Linker error", "parent": goal.id,
                   "body": "ld: symbol _sqlite3_open not found in src/app/memory/notes.rs"}),
        )
        .await;
        let elsewhere = add(
            &store,
            json!({"kind": "fact", "title": "Other project", "project": "other", "body": "_sqlite3_open again"}),
        )
        .await;

        assert_eq!(
            find_ids(&store, json!({"query": "src/app/memory/notes.rs"})).await,
            [fact.id]
        );
        assert_eq!(
            find_ids(&store, json!({"query": "LINKER"})).await,
            [fact.id]
        );
        assert_eq!(
            find_ids(
                &store,
                json!({"query": "_sqlite3_open", "project": "other"})
            )
            .await,
            [elsewhere.id]
        );
        assert_eq!(
            find_ids(&store, json!({"query": "_sqlite3_open", "project": "*"}))
                .await
                .len(),
            2
        );
        assert!(
            find_ids(&store, json!({"query": "linker", "kind": "decision"}))
                .await
                .is_empty()
        );
        assert_eq!(
            find_ids(&store, json!({"query": "linker", "under": goal.id})).await,
            [fact.id]
        );
        assert!(
            find_ids(&store, json!({"query": "linker", "under": elsewhere.id}))
                .await
                .is_empty()
        );
    }

    #[tokio::test]
    async fn find_falls_back_to_any_term() {
        let store = NoteStore::in_memory();
        let note = add(
            &store,
            json!({"kind": "fact", "title": "tokio runtime panics"}),
        )
        .await;
        let query = serde_json::from_value(json!({"query": "tokio deadlock"})).unwrap();
        let result = store.find(query, scope()).await.unwrap();
        assert_eq!(result.matched, "any");
        assert_eq!(result.hits[0].note.id, note.id);
    }

    #[tokio::test]
    async fn find_treats_fts_syntax_as_plain_text() {
        let store = NoteStore::in_memory();
        add(&store, json!({"kind": "fact", "title": "alpha beta"})).await;
        for hostile in [
            "alpha\" OR \"beta",
            "NEAR(alpha beta)",
            "alpha*",
            "-alpha",
            "a AND",
            "x\"\"y",
            "(",
            "\"",
        ] {
            let query: FindQuery = serde_json::from_value(json!({"query": hostile})).unwrap();
            let searchable = hostile.chars().any(char::is_alphanumeric);
            assert_eq!(
                store.find(query, scope()).await.is_ok(),
                searchable,
                "query: {hostile}"
            );
        }
        let no_terms = serde_json::from_value(json!({"query": "\"\" -- **"})).unwrap();
        assert!(matches!(
            store.find(no_terms, scope()).await,
            Err(MemoryError::Invalid(_))
        ));
        // As a column filter this would match, as a phrase it must not.
        assert!(
            find_ids(&store, json!({"query": "title:alpha"}))
                .await
                .is_empty()
        );
    }

    #[tokio::test]
    async fn index_follows_updates_and_deletes() {
        let store = NoteStore::in_memory();
        let note = add(&store, json!({"kind": "fact", "title": "before"})).await;
        let patch = serde_json::from_value(json!({"id": note.id, "title": "after"})).unwrap();
        store.update(patch).await.unwrap();
        assert!(
            find_ids(&store, json!({"query": "before"}))
                .await
                .is_empty()
        );
        assert_eq!(find_ids(&store, json!({"query": "after"})).await, [note.id]);

        store.forget(note.id, false).await.unwrap();
        assert!(find_ids(&store, json!({"query": "after"})).await.is_empty());
    }

    #[tokio::test]
    async fn update_checks_status_and_empty_patch() {
        let store = NoteStore::in_memory();
        let fact = add(
            &store,
            json!({"kind": "fact", "title": "f", "body": "kept"}),
        )
        .await;

        let empty = serde_json::from_value(json!({"id": fact.id})).unwrap();
        assert!(matches!(
            store.update(empty).await,
            Err(MemoryError::Invalid(_))
        ));
        let bad = serde_json::from_value(json!({"id": fact.id, "status": "failed"})).unwrap();
        assert!(matches!(
            store.update(bad).await,
            Err(MemoryError::Invalid(_))
        ));
        let missing = serde_json::from_value(json!({"id": 999, "title": "x"})).unwrap();
        assert!(matches!(
            store.update(missing).await,
            Err(MemoryError::NotFound(_))
        ));

        let good = serde_json::from_value(json!({"id": fact.id, "status": "dropped"})).unwrap();
        let updated = store.update(good).await.unwrap();
        assert_eq!(updated.status, Status::Dropped);
        assert_eq!(updated.body, "kept");
    }

    #[tokio::test]
    async fn reparenting_cannot_create_a_cycle() {
        let store = NoteStore::in_memory();
        let goal = add(&store, json!({"kind": "goal", "title": "G"})).await;
        let a = add(
            &store,
            json!({"kind": "step", "title": "A", "parent": goal.id}),
        )
        .await;
        let b = add(
            &store,
            json!({"kind": "step", "title": "B", "parent": a.id}),
        )
        .await;

        let cycle = serde_json::from_value(json!({"id": a.id, "parent": b.id})).unwrap();
        assert!(matches!(
            store.update(cycle).await,
            Err(MemoryError::Conflict(_))
        ));
        let itself = serde_json::from_value(json!({"id": a.id, "parent": a.id})).unwrap();
        assert!(matches!(
            store.update(itself).await,
            Err(MemoryError::Conflict(_))
        ));
        let goal_child = serde_json::from_value(json!({"id": goal.id, "parent": a.id})).unwrap();
        assert!(matches!(
            store.update(goal_child).await,
            Err(MemoryError::Invalid(_))
        ));

        let moved = serde_json::from_value(json!({"id": b.id, "parent": goal.id})).unwrap();
        assert_eq!(store.update(moved).await.unwrap().parent, Some(goal.id));
    }

    #[tokio::test]
    async fn the_tree_has_a_depth_limit_that_moves_respect() {
        use crate::app::memory::db::MAX_TREE_DEPTH;
        let store = NoteStore::in_memory();
        let mut chain = vec![
            add(&store, json!({"kind": "goal", "title": "root"}))
                .await
                .id,
        ];
        for level in 1..MAX_TREE_DEPTH {
            let parent = chain[chain.len() - 1];
            chain.push(
                add(
                    &store,
                    json!({"kind": "step", "title": format!("level {level}"), "parent": parent}),
                )
                .await
                .id,
            );
        }
        let deepest = chain[chain.len() - 1];
        let too_deep = try_add(
            &store,
            json!({"kind": "step", "title": "one more", "parent": deepest}),
        )
        .await;
        assert!(matches!(too_deep, Err(MemoryError::Invalid(_))));

        let top = add(
            &store,
            json!({"kind": "step", "title": "top", "parent": chain[0]}),
        )
        .await
        .id;
        add(
            &store,
            json!({"kind": "step", "title": "leaf", "parent": top}),
        )
        .await;
        let sinks = patch(&store, json!({"id": top, "parent": chain[chain.len() - 2]})).await;
        assert!(matches!(sinks, Err(MemoryError::Invalid(_))));
        assert!(
            patch(&store, json!({"id": top, "parent": chain[1]}))
                .await
                .is_ok()
        );

        assert_eq!(
            store.forget(chain[0], true).await.unwrap(),
            MAX_TREE_DEPTH + 2
        );
        assert!(find_ids(&store, json!({"query": "level"})).await.is_empty());
    }

    #[tokio::test]
    async fn a_note_cannot_move_to_another_project_or_link_into_a_cycle_at_birth() {
        let store = NoteStore::in_memory();
        let here = add(&store, json!({"kind": "step", "title": "here"}))
            .await
            .id;
        let there = add(
            &store,
            json!({"kind": "step", "title": "there", "project": "other"}),
        )
        .await
        .id;
        let moved = patch(&store, json!({"id": here, "parent": there})).await;
        assert!(matches!(moved, Err(MemoryError::Invalid(_))));

        let a = add(&store, json!({"kind": "step", "title": "a"})).await.id;
        let b = add(
            &store,
            json!({"kind": "step", "title": "b", "links": [{"kind": "depends_on", "to": a}]}),
        )
        .await
        .id;
        assert_eq!(store.get(b).await.unwrap().links.len(), 1);
        store.link(edge(a, b, EdgeKind::RelatesTo)).await.unwrap();
        let closing = store.link(edge(a, b, EdgeKind::DependsOn)).await;
        assert!(matches!(closing, Err(MemoryError::Conflict(_))));
    }

    #[tokio::test]
    async fn control_characters_in_a_query_are_invalid_input() {
        for hostile in ["a\u{0}b", "a\nb", "tab\there"] {
            let query = serde_json::from_value::<FindQuery>(json!({ "query": hostile }));
            assert!(query.is_err(), "{hostile:?}");
        }
    }

    #[tokio::test]
    async fn links_reject_self_missing_and_dependency_cycles() {
        let store = NoteStore::in_memory();
        let a = add(&store, json!({"kind": "step", "title": "A"})).await.id;
        let b = add(&store, json!({"kind": "step", "title": "B"})).await.id;
        let c = add(&store, json!({"kind": "step", "title": "C"})).await.id;

        assert!(matches!(
            store.link(edge(a, a, EdgeKind::RelatesTo)).await,
            Err(MemoryError::Invalid(_))
        ));
        assert!(matches!(
            store.link(edge(a, 999, EdgeKind::RelatesTo)).await,
            Err(MemoryError::NotFound(_))
        ));

        store.link(edge(a, b, EdgeKind::DependsOn)).await.unwrap();
        store.link(edge(b, c, EdgeKind::DependsOn)).await.unwrap();
        assert!(matches!(
            store.link(edge(c, a, EdgeKind::DependsOn)).await,
            Err(MemoryError::Conflict(_))
        ));
        store.link(edge(c, a, EdgeKind::RelatesTo)).await.unwrap();
        store.link(edge(a, b, EdgeKind::DependsOn)).await.unwrap();

        assert!(store.unlink(edge(a, b, EdgeKind::DependsOn)).await.unwrap());
        assert!(!store.unlink(edge(a, b, EdgeKind::DependsOn)).await.unwrap());
        store.link(edge(c, a, EdgeKind::DependsOn)).await.unwrap();
    }

    #[tokio::test]
    async fn graph_is_depth_first_and_depth_limited() {
        let store = NoteStore::in_memory();
        let goal = add(&store, json!({"kind": "goal", "title": "G"})).await.id;
        let s1 = add(
            &store,
            json!({"kind": "step", "title": "S1", "parent": goal}),
        )
        .await
        .id;
        let s2 = add(
            &store,
            json!({"kind": "step", "title": "S2", "parent": goal}),
        )
        .await
        .id;
        let try1 = add(
            &store,
            json!({"kind": "attempt", "title": "T", "status": "failed", "parent": s1,
                   "links": [{"kind": "relates_to", "to": s2}]}),
        )
        .await
        .id;
        store.link(edge(s2, s1, EdgeKind::DependsOn)).await.unwrap();

        let full = store.graph(goal, 8).await.unwrap();
        let order: Vec<(NoteId, i64)> = full.nodes.iter().map(|n| (n.id, n.depth)).collect();
        assert_eq!(order, [(goal, 0), (s1, 1), (try1, 2), (s2, 1)]);
        assert_eq!(full.edges.len(), 2);
        assert!(!full.truncated);

        let shallow = store.graph(goal, 1).await.unwrap();
        assert_eq!(shallow.nodes.len(), 3);
        assert!(matches!(
            store.graph(999, 8).await,
            Err(MemoryError::NotFound(_))
        ));
    }

    #[tokio::test]
    async fn snapshot_lists_open_work_per_project() {
        let store = NoteStore::in_memory();
        let goal = add(&store, json!({"kind": "goal", "title": "G"})).await.id;
        add(
            &store,
            json!({"kind": "goal", "title": "Done", "status": "done"}),
        )
        .await;
        add(
            &store,
            json!({"kind": "goal", "title": "Elsewhere", "project": "other"}),
        )
        .await;
        let step = add(
            &store,
            json!({"kind": "step", "title": "S", "status": "active", "parent": goal}),
        )
        .await
        .id;
        add(
            &store,
            json!({"kind": "step", "title": "Later", "parent": goal}),
        )
        .await;
        let question = add(
            &store,
            json!({"kind": "question", "title": "Q", "parent": goal}),
        )
        .await
        .id;

        let snapshot = store.snapshot(scope()).await.unwrap();
        assert_eq!(
            snapshot.goals.iter().map(|n| n.id).collect::<Vec<_>>(),
            [goal]
        );
        assert_eq!(
            snapshot
                .active_steps
                .iter()
                .map(|n| n.id)
                .collect::<Vec<_>>(),
            [step]
        );
        assert_eq!(
            snapshot
                .open_questions
                .iter()
                .map(|n| n.id)
                .collect::<Vec<_>>(),
            [question]
        );
        assert_eq!(snapshot.recent[0].id, question);
        assert_eq!(snapshot.total_notes, 5);
        assert_eq!(store.snapshot(Scope::All).await.unwrap().total_notes, 6);
    }

    #[tokio::test]
    async fn forget_needs_recursive_for_a_subtree() {
        let store = NoteStore::in_memory();
        let goal = add(&store, json!({"kind": "goal", "title": "G"})).await.id;
        let step = add(
            &store,
            json!({"kind": "step", "title": "S", "parent": goal}),
        )
        .await
        .id;
        let other = add(&store, json!({"kind": "fact", "title": "F"})).await.id;
        store
            .link(edge(other, step, EdgeKind::Supports))
            .await
            .unwrap();

        assert!(matches!(
            store.forget(goal, false).await,
            Err(MemoryError::Conflict(_))
        ));
        assert_eq!(store.forget(goal, true).await.unwrap(), 2);
        assert!(matches!(
            store.get(step).await,
            Err(MemoryError::NotFound(_))
        ));
        assert!(store.get(other).await.unwrap().links.is_empty());
        assert!(matches!(
            store.forget(goal, true).await,
            Err(MemoryError::NotFound(_))
        ));
    }

    #[tokio::test]
    async fn get_returns_children_and_both_link_directions() {
        let store = NoteStore::in_memory();
        let a = add(&store, json!({"kind": "step", "title": "A"})).await.id;
        let child = add(
            &store,
            json!({"kind": "fact", "title": "child", "parent": a}),
        )
        .await
        .id;
        let b = add(
            &store,
            json!({"kind": "step", "title": "B", "links": [{"kind": "depends_on", "to": a}]}),
        )
        .await
        .id;
        let c = add(&store, json!({"kind": "step", "title": "C"})).await.id;
        store.link(edge(a, c, EdgeKind::RelatesTo)).await.unwrap();

        let detail = store.get(a).await.unwrap();
        assert_eq!(
            detail.children.iter().map(|n| n.id).collect::<Vec<_>>(),
            [child]
        );
        let mut peers: Vec<NoteId> = detail.links.iter().map(|l| l.peer.id).collect();
        peers.sort_unstable();
        assert_eq!(peers, [b, c]);
    }

    #[test]
    fn reopening_a_database_keeps_its_notes() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("brain.sqlite");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .build()
            .unwrap();
        let id = runtime.block_on(async {
            let store = NoteStore::new(Db::open(&path).unwrap());
            add(&store, json!({"kind": "fact", "title": "persisted"}))
                .await
                .id
        });
        let reopened = NoteStore::new(Db::open(&path).unwrap());
        assert_eq!(
            runtime.block_on(reopened.get(id)).unwrap().note.title,
            "persisted"
        );
    }
}
