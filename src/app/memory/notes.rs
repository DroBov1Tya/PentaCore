use rusqlite::{Connection, OptionalExtension, Row, TransactionBehavior, params};

use super::db::{Db, Tree};
use super::journal::{self, Entry, NoteState, Op, RecordType};
use super::model::{
    Attribution, Checkpoint, CreatedNote, Edge, EdgeKind, EntityId, FindQuery, FindResult, Graph,
    GraphNode, Hit, Limit, LinkedNote, MAX_BODY_BYTES, MemoryError, NewCheckpoint, NewNote, Note,
    NoteBrief, NoteDetail, NoteId, NoteKind, NotePatch, ProjectName, Scope, Snapshot, TaskBrief,
    invalid, names, now, tag_strings,
};

const DEFAULT_FIND_LIMIT: usize = 10;
const MAX_GRAPH_NODES: usize = 500;
const MAX_GRAPH_EDGES: usize = 1000;
const MAX_CHILDREN_LISTED: i64 = 200;
const SNAPSHOT_SECTION_LIMIT: i64 = 30;
const SNAPSHOT_RECENT_LIMIT: i64 = 20;
const SIMILAR_LIMIT: i64 = 3;
const CHECKPOINTS_KEPT: i64 = 20;

const NOTE_COLUMNS: &str = "id, project, kind, status, title, body, tags, parent_id, created_at, updated_at, entity_id, author, revision";
pub(super) const BRIEF_COLUMNS: &str =
    "n.id, n.project, n.kind, n.status, n.title, n.parent_id, n.updated_at";

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
                if insert_edge(&tx, &edge)? {
                    log_edge(&tx, &edge, Op::Linked, &Attribution::default())?;
                }
                tx.commit()?;
                Ok(edge)
            })
            .await
    }

    pub async fn unlink(&self, edge: Edge) -> Result<bool> {
        self.db
            .run(move |conn| {
                let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
                let removed = tx.execute(
                    "DELETE FROM edges WHERE src = ?1 AND dst = ?2 AND kind = ?3",
                    params![edge.src, edge.dst, edge.kind],
                )?;
                if removed > 0 {
                    log_edge(&tx, &edge, Op::Unlinked, &Attribution::default())?;
                }
                tx.commit()?;
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

    pub async fn forget(&self, id: NoteId, recursive: bool, who: Attribution) -> Result<i64> {
        self.db
            .run(move |conn| forget(conn, id, recursive, &who))
            .await
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
        revision: row.get(12)?,
    })
}

pub(super) fn brief_from_row(row: &Row<'_>) -> rusqlite::Result<NoteBrief> {
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

pub(super) fn not_found(id: NoteId) -> MemoryError {
    MemoryError::NotFound(format!("note {id}"))
}

pub(super) fn load(conn: &Connection, id: NoteId) -> Result<Note> {
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

pub(super) fn create(
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
    if let Some(task) = input.task {
        check_task(&tx, task, &project)?;
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
    let who = Attribution::new(input.author, input.run, Some(task_of(&tx, id, input.task)?));
    for link in input.links.as_slice() {
        let edge = Edge {
            src: id,
            dst: link.to,
            kind: link.kind,
        };
        insert_edge(&tx, &edge)?;
        log_edge(&tx, &edge, Op::Linked, &who)?;
    }
    let note = load(&tx, id)?;
    save_revision(&tx, &note, &who)?;
    journal::record(
        &tx,
        Entry::new(&note.project, RecordType::Note, id, Op::Created, &who).revision(note.revision),
    )?;
    let similar = similar_notes(&tx, &note)?;
    tx.commit()?;
    Ok(CreatedNote { note, similar })
}

fn save_revision(conn: &Connection, note: &Note, who: &Attribution) -> Result<()> {
    journal::save_note_revision(
        conn,
        &NoteState {
            id: note.id,
            revision: note.revision,
            status: note.status.as_str(),
            title: &note.title,
            body: &note.body,
            tags: &note.tags.join(" "),
            parent: note.parent,
            entity: note.entity,
        },
        who,
    )
}

// A change belongs to the task it names, else to the root of the note's tree.
fn task_of(conn: &Connection, note: NoteId, named: Option<NoteId>) -> Result<NoteId> {
    if let Some(task) = named {
        return Ok(task);
    }
    Ok(conn.query_row(
        "WITH RECURSIVE up (id, parent_id) AS (
             SELECT id, parent_id FROM notes WHERE id = ?1
             UNION ALL
             SELECT n.id, n.parent_id FROM notes n JOIN up ON n.id = up.parent_id
         )
         SELECT id FROM up WHERE parent_id IS NULL",
        [note],
        |row| row.get(0),
    )?)
}

pub(super) fn check_task(conn: &Connection, task: NoteId, project: &str) -> Result<()> {
    let task_project = load(conn, task)?.project;
    if task_project != project {
        return Err(invalid(format!(
            "task {task} belongs to project '{task_project}', not '{project}'"
        )));
    }
    Ok(())
}

fn log_edge(conn: &Connection, edge: &Edge, op: Op, who: &Attribution) -> Result<()> {
    let project = load(conn, edge.src)?.project;
    let who = Attribution {
        task: Some(task_of(conn, edge.src, who.task)?),
        ..who.clone()
    };
    journal::record(
        conn,
        Entry::new(&project, RecordType::Note, edge.src, op, &who)
            .fields([format!("{}:{}", edge.kind, edge.dst)]),
    )?;
    Ok(())
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

// Returns false when the link was already there.
fn insert_edge(conn: &Connection, edge: &Edge) -> Result<bool> {
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
    let inserted = conn.execute(
        "INSERT OR IGNORE INTO edges (src, dst, kind, created_at) VALUES (?1, ?2, ?3, ?4)",
        params![edge.src, edge.dst, edge.kind, now()],
    )?;
    Ok(inserted > 0)
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

pub(super) fn update(conn: &mut Connection, patch: NotePatch) -> Result<Note> {
    let NotePatch {
        id,
        title,
        body,
        status,
        tags,
        parent,
        append,
        entity,
        author,
        run,
        expected_revision,
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
    if let Some(expected) = expected_revision
        && expected != current.revision
    {
        return Err(MemoryError::Conflict(format!(
            "note {id} is at revision {}, not {expected}; read it again before changing it",
            current.revision
        )));
    }
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
        (None, None) => current.body.clone(),
    };
    let title = title.map_or_else(|| current.title.clone(), String::from);
    let tags = tags.map_or_else(
        || current.tags.join(" "),
        |tags| tag_strings(&tags).join(" "),
    );
    let status = status.unwrap_or(current.status);
    let parent = parent.or(current.parent);
    let entity = entity.or(current.entity);

    let changed: Vec<&str> = [
        ("title", title != current.title),
        ("body", body != current.body),
        ("tags", tags != current.tags.join(" ")),
        ("status", status != current.status),
        ("parent", parent != current.parent),
        ("entity", entity != current.entity),
    ]
    .into_iter()
    .filter_map(|(field, differs)| differs.then_some(field))
    .collect();
    // A write that changes nothing leaves no revision behind.
    if changed.is_empty() {
        return Ok(current);
    }

    tx.execute(
        "UPDATE notes
         SET title = ?2, body = ?3, tags = ?4, status = ?5, parent_id = ?6, entity_id = ?7,
             updated_at = ?8, revision = ?9
         WHERE id = ?1",
        params![
            id,
            title,
            body,
            tags,
            status,
            parent,
            entity,
            now(),
            current.revision + 1,
        ],
    )?;
    let note = load(&tx, id)?;
    let who = Attribution::new(author, run, Some(task_of(&tx, id, None)?));
    save_revision(&tx, &note, &who)?;
    journal::record(
        &tx,
        Entry::new(&note.project, RecordType::Note, id, Op::Updated, &who)
            .revision(note.revision)
            .fields(changed),
    )?;
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
        tasks: unfinished_tasks(conn, project)?,
    })
}

fn unfinished_tasks(conn: &Connection, project: Option<&str>) -> Result<Vec<TaskBrief>> {
    let goals: Vec<(TaskBrief, i64)> = conn
        .prepare(
            "SELECT n.id, n.project, n.status, n.title, c.created_at, coalesce(c.journal_id, 0)
             FROM notes n
             LEFT JOIN checkpoints c
                    ON c.id = (SELECT max(id) FROM checkpoints WHERE task_id = n.id)
             WHERE (?1 IS NULL OR n.project = ?1)
               AND n.kind = 'goal' AND n.status IN ('open', 'active')
             ORDER BY n.updated_at DESC, n.id DESC
             LIMIT ?2",
        )?
        .query_map(params![project, SNAPSHOT_SECTION_LIMIT], |row| {
            Ok((
                TaskBrief {
                    id: row.get(0)?,
                    project: row.get(1)?,
                    status: row.get(2)?,
                    title: row.get(3)?,
                    checkpoint_at: row.get(4)?,
                    changes_since_checkpoint: 0,
                },
                row.get(5)?,
            ))
        })?
        .collect::<rusqlite::Result<_>>()?;
    goals
        .into_iter()
        .map(|(mut task, position)| {
            task.changes_since_checkpoint = journal::task_changes(conn, task.id, position, 0)?.0;
            Ok(task)
        })
        .collect()
}

pub(super) const CHECKPOINT_COLUMNS: &str =
    "id, project, summary, author, created_at, run, task_id, journal_id";

pub(super) fn checkpoint_from_row(row: &Row<'_>) -> rusqlite::Result<Checkpoint> {
    Ok(Checkpoint {
        id: row.get(0)?,
        project: row.get(1)?,
        summary: row.get(2)?,
        author: row.get(3)?,
        created_at: row.get(4)?,
        run: row.get(5)?,
        task: row.get(6)?,
        journal_id: row.get(7)?,
    })
}

// Keeps only the latest checkpoints of each task, and of the project as a whole.
fn save_checkpoint(
    conn: &mut Connection,
    input: NewCheckpoint,
    default_project: ProjectName,
) -> Result<Checkpoint> {
    if input.summary.as_str().trim().is_empty() {
        return Err(invalid("summary must not be empty"));
    }
    let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
    // A task's checkpoint lives in the task's project.
    let project: String = match (input.task, input.project) {
        (Some(task), Some(project)) => {
            check_task(&tx, task, project.as_str())?;
            project.into()
        }
        (Some(task), None) => load(&tx, task)?.project,
        (None, project) => project.unwrap_or(default_project).into(),
    };
    let who = Attribution::new(input.author, input.run, input.task);
    tx.execute(
        "INSERT INTO checkpoints (project, summary, author, run, task_id, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![
            project,
            input.summary.as_str(),
            who.author,
            who.run,
            who.task,
            now()
        ],
    )?;
    let id = tx.last_insert_rowid();
    // The checkpoint's own entry is not a change made after it.
    let position = journal::record(
        &tx,
        Entry::new(&project, RecordType::Checkpoint, id, Op::Created, &who),
    )?;
    tx.execute(
        "UPDATE checkpoints SET journal_id = ?2 WHERE id = ?1",
        params![id, position],
    )?;
    tx.execute(
        "DELETE FROM checkpoints
         WHERE project = ?1 AND task_id IS ?3
           AND id NOT IN (SELECT id FROM checkpoints WHERE project = ?1 AND task_id IS ?3
                          ORDER BY id DESC LIMIT ?2)",
        params![project, CHECKPOINTS_KEPT, who.task],
    )?;
    let checkpoint = tx.query_row(
        &format!("SELECT {CHECKPOINT_COLUMNS} FROM checkpoints WHERE id = ?1"),
        [id],
        checkpoint_from_row,
    )?;
    tx.commit()?;
    Ok(checkpoint)
}

// Removes the notes with every stored revision. What stays is one journal
// entry per note saying that it existed and who purged it.
fn forget(conn: &mut Connection, id: NoteId, recursive: bool, who: &Attribution) -> Result<i64> {
    let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
    ensure_exists(&tx, id)?;
    let doomed: Vec<(NoteId, String)> = tx
        .prepare(
            "WITH RECURSIVE sub (id) AS (
                 SELECT ?1
                 UNION
                 SELECT c.id FROM notes c JOIN sub ON c.parent_id = sub.id
             )
             SELECT n.id, n.project FROM sub JOIN notes n ON n.id = sub.id",
        )?
        .query_map([id], |row| Ok((row.get(0)?, row.get(1)?)))?
        .collect::<rusqlite::Result<_>>()?;
    let subtree_size = doomed.len() as i64;
    if subtree_size > 1 && !recursive {
        return Err(MemoryError::Conflict(format!(
            "note {id} has {} notes beneath it; pass recursive=true to delete them too",
            subtree_size - 1
        )));
    }
    let who = Attribution {
        task: Some(task_of(&tx, id, who.task)?),
        ..who.clone()
    };
    for (note, project) in &doomed {
        journal::record(
            &tx,
            Entry::new(project, RecordType::Note, note, Op::Purged, &who),
        )?;
        journal::unindex(&tx, RecordType::Note, note)?;
    }
    // Children, edges, revisions and task checkpoints go through ON DELETE CASCADE.
    tx.execute("DELETE FROM notes WHERE id = ?1", [id])?;
    tx.commit()?;
    Ok(subtree_size)
}

#[cfg(test)]
#[path = "tests/notes.rs"]
mod tests;
