use rusqlite::{Connection, OptionalExtension, Row, TransactionBehavior, params};
use serde::{Deserialize, Serialize};
use std::fmt;
use tokio::sync::OnceCell;
use uuid::Uuid;

use super::db::Db;
use super::index::{LegacyConcept, Semantic, embedding_text};
use super::journal::{self, ConceptState, Entry, Op, Origin, RecordType};
use super::model::{
    AtMost, Attribution, Author, Body, MemoryError, NoteId, ProjectName, RunId, Tag, Tags, Title,
    invalid, now, tag_strings,
};
use super::words;

const SIMILAR_LIMIT: usize = 3;
// Tuned by eye on paraphrases. A hint for the caller, not a verdict.
const SIMILAR_MIN_SCORE: f32 = 0.6;
const LEGACY_IMPORTED: &str = "legacy_concepts_imported";
const CONCEPT_COLUMNS: &str = "id, project, title, content, tags, sources, archived, author, revision, created_at, updated_at";

type Result<T> = std::result::Result<T, MemoryError>;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(try_from = "String")]
pub struct ConceptId(Uuid);

impl TryFrom<String> for ConceptId {
    type Error = String;

    fn try_from(value: String) -> std::result::Result<Self, String> {
        Uuid::parse_str(&value)
            .map(Self)
            .map_err(|_| "concept id must be a UUID".to_string())
    }
}

impl fmt::Display for ConceptId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

// Soft reference: the notes may be deleted later.
pub type Sources = AtMost<NoteId, 16>;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NewConcept {
    pub title: Title,
    pub content: Body,
    #[serde(default)]
    pub tags: Tags,
    pub project: Option<ProjectName>,
    #[serde(default)]
    pub sources: Sources,
    pub author: Option<Author>,
    pub run: Option<RunId>,
    pub task: Option<NoteId>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConceptPatch {
    pub id: ConceptId,
    pub title: Option<Title>,
    pub content: Option<Body>,
    pub tags: Option<Tags>,
    pub archived: Option<bool>,
    pub author: Option<Author>,
    pub run: Option<RunId>,
    pub expected_revision: Option<i64>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ForgetConcept {
    pub id: ConceptId,
    pub author: Option<Author>,
    pub run: Option<RunId>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Concept {
    pub id: String,
    pub project: String,
    pub title: String,
    pub content: String,
    pub tags: Vec<String>,
    pub sources: Vec<NoteId>,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub archived: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub author: Option<String>,
    pub revision: i64,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Serialize)]
pub struct MemorizedConcept {
    #[serde(flatten)]
    pub concept: Concept,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub similar: Vec<SimilarConcept>,
}

#[derive(Debug, Serialize)]
pub struct SimilarConcept {
    pub id: String,
    pub title: String,
    pub score: f32,
}

// The score clients have always seen: 1 / (1 + squared distance of unit vectors).
pub fn score_of(similarity: f32) -> f32 {
    1.0 / (1.0 + 2.0 * (1.0 - similarity).max(0.0))
}

pub struct ConceptStore {
    db: Db,
    semantic: Semantic,
    imported: OnceCell<()>,
}

impl ConceptStore {
    pub fn new(db: Db, semantic: Semantic) -> Self {
        Self {
            db,
            semantic,
            imported: OnceCell::new(),
        }
    }

    pub async fn memorize(
        &self,
        input: NewConcept,
        default_project: ProjectName,
    ) -> Result<MemorizedConcept> {
        require_content(&input.content)?;
        self.import_legacy().await?;
        let concept = self
            .db
            .run(move |conn| insert(conn, input, default_project))
            .await?;
        let similar = match self.similar_by_meaning(&concept).await {
            Ok(similar) => similar,
            // The concept is stored either way; it is embedded once the model loads.
            Err(MemoryError::Internal(cause)) => {
                tracing::warn!("concept stored without a vector: {cause:#}");
                let like = concept.clone();
                self.db
                    .run(move |conn| similar_by_words(conn, &like))
                    .await?
            }
            Err(other) => return Err(other),
        };
        Ok(MemorizedConcept { concept, similar })
    }

    async fn similar_by_meaning(&self, concept: &Concept) -> Result<Vec<SimilarConcept>> {
        let vector = self
            .semantic
            .index_now(
                RecordType::Concept,
                &concept.id,
                &concept.project,
                concept.revision,
                embedding_text(&concept.title, &concept.content, &concept.tags),
            )
            .await?;
        let neighbors = self
            .semantic
            .nearest(
                &vector,
                Some(&concept.project),
                Some(RecordType::Concept),
                SIMILAR_LIMIT + 1,
            )
            .await?;
        let candidates: Vec<(String, f32)> = neighbors
            .into_iter()
            .filter(|neighbor| neighbor.id != concept.id)
            .map(|neighbor| (neighbor.id, score_of(neighbor.similarity)))
            .filter(|(_, score)| *score >= SIMILAR_MIN_SCORE)
            .take(SIMILAR_LIMIT)
            .collect();
        self.db
            .run(move |conn| {
                let mut similar = Vec::new();
                for (id, score) in candidates {
                    if let Some(found) = load_live(conn, &id)? {
                        similar.push(SimilarConcept {
                            id,
                            title: found.title,
                            score,
                        });
                    }
                }
                Ok(similar)
            })
            .await
    }

    pub async fn update(&self, patch: ConceptPatch) -> Result<Concept> {
        if let Some(content) = &patch.content {
            require_content(content)?;
        }
        self.import_legacy().await?;
        let concept = self.db.run(move |conn| update(conn, patch)).await?;
        self.semantic.catch_up();
        Ok(concept)
    }

    #[cfg(test)]
    pub async fn get(&self, id: ConceptId) -> Result<Concept> {
        self.import_legacy().await?;
        self.db
            .run(move |conn| load(conn, &id.to_string())?.ok_or_else(|| not_found(id)))
            .await
    }

    // Removes the concept with every stored revision and its vector.
    pub async fn forget(&self, request: ForgetConcept) -> Result<()> {
        self.import_legacy().await?;
        self.db.run(move |conn| forget(conn, request)).await?;
        if let Err(error) = self.semantic.drop_purged().await {
            // Still queued; removed the next time the index is used.
            tracing::warn!("purged concept is still in the search index: {error}");
        }
        Ok(())
    }

    // Copies the concepts an earlier version kept in LanceDB into SQLite,
    // once. Their earlier history is unknown and is marked as such.
    pub async fn import_legacy(&self) -> Result<()> {
        let imported = self
            .imported
            .get_or_try_init(|| async {
                let done = self
                    .db
                    .run(|conn| {
                        Ok(conn
                            .query_row(
                                "SELECT 1 FROM meta WHERE key = ?1",
                                [LEGACY_IMPORTED],
                                |_| Ok(()),
                            )
                            .optional()?
                            .is_some())
                    })
                    .await?;
                if done {
                    return Ok(());
                }
                let legacy = self.semantic.legacy_concepts().await?.unwrap_or_default();
                let imported = self.db.run(move |conn| import(conn, legacy)).await?;
                if imported > 0 {
                    tracing::info!(imported, "concepts moved from the vector store into SQLite");
                }
                Ok(())
            })
            .await
            .copied();
        match imported {
            // An unreadable old store must not take the rest of the memory
            // down with it. Nothing is marked as done, so the next call retries.
            Err(MemoryError::Internal(cause)) => {
                tracing::warn!("concepts of an earlier version could not be read: {cause:#}");
                Ok(())
            }
            other => other,
        }
    }
}

fn concept_from_row(row: &Row<'_>) -> rusqlite::Result<Concept> {
    let tags: String = row.get(4)?;
    let sources: String = row.get(5)?;
    Ok(Concept {
        id: row.get(0)?,
        project: row.get(1)?,
        title: row.get(2)?,
        content: row.get(3)?,
        tags: tags.split_whitespace().map(String::from).collect(),
        sources: serde_json::from_str(&sources).unwrap_or_default(),
        archived: row.get(6)?,
        author: row.get(7)?,
        revision: row.get(8)?,
        created_at: row.get(9)?,
        updated_at: row.get(10)?,
    })
}

pub(super) fn load(conn: &Connection, id: &str) -> Result<Option<Concept>> {
    Ok(conn
        .query_row(
            &format!("SELECT {CONCEPT_COLUMNS} FROM concepts WHERE id = ?1"),
            [id],
            concept_from_row,
        )
        .optional()?)
}

fn load_live(conn: &Connection, id: &str) -> Result<Option<Concept>> {
    Ok(load(conn, id)?.filter(|concept| !concept.archived))
}

fn not_found(id: ConceptId) -> MemoryError {
    MemoryError::NotFound(format!("concept {id}"))
}

fn require_content(content: &Body) -> Result<()> {
    if content.as_str().trim().is_empty() {
        return Err(invalid("content must not be empty"));
    }
    Ok(())
}

fn save_revision(
    conn: &Connection,
    concept: &Concept,
    who: &Attribution,
    origin: Origin,
) -> Result<()> {
    let sources = serde_json::to_string(&concept.sources)
        .map_err(|error| MemoryError::Internal(error.into()))?;
    journal::save_concept_revision(
        conn,
        &ConceptState {
            id: &concept.id,
            revision: concept.revision,
            title: &concept.title,
            content: &concept.content,
            tags: &concept.tags.join(" "),
            sources: &sources,
            archived: concept.archived,
        },
        who,
        origin,
        &concept.updated_at,
    )
}

fn insert(
    conn: &mut Connection,
    input: NewConcept,
    default_project: ProjectName,
) -> Result<Concept> {
    let project: String = input.project.unwrap_or(default_project).into();
    let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
    if let Some(task) = input.task {
        super::notes::check_task(&tx, task, &project)?;
    }
    let who = Attribution::new(input.author, input.run, input.task);
    let now = now();
    let concept = Concept {
        id: Uuid::new_v4().to_string(),
        project,
        title: input.title.into(),
        content: input.content.into(),
        tags: tag_strings(&input.tags)
            .into_iter()
            .map(String::from)
            .collect(),
        sources: input.sources.as_slice().to_vec(),
        archived: false,
        author: who.author.clone(),
        revision: 1,
        created_at: now.clone(),
        updated_at: now,
    };
    write_new(&tx, &concept, &who, Origin::Recorded, Op::Created)?;
    tx.commit()?;
    Ok(concept)
}

fn write_new(
    conn: &Connection,
    concept: &Concept,
    who: &Attribution,
    origin: Origin,
    op: Op,
) -> Result<bool> {
    let sources = serde_json::to_string(&concept.sources)
        .map_err(|error| MemoryError::Internal(error.into()))?;
    let inserted = conn.execute(
        "INSERT OR IGNORE INTO concepts
             (id, project, title, content, tags, sources, archived, author, revision, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, 0, ?7, 1, ?8, ?9)",
        params![
            concept.id,
            concept.project,
            concept.title,
            concept.content,
            concept.tags.join(" "),
            sources,
            concept.author,
            concept.created_at,
            concept.updated_at,
        ],
    )?;
    if inserted == 0 {
        return Ok(false);
    }
    save_revision(conn, concept, who, origin)?;
    journal::record(
        conn,
        Entry::new(&concept.project, RecordType::Concept, &concept.id, op, who)
            .revision(1)
            .origin(origin),
    )?;
    Ok(true)
}

// The same limits a concept written today must meet.
fn fits_todays_limits(old: &LegacyConcept) -> bool {
    Uuid::parse_str(&old.id).is_ok()
        && ProjectName::try_from(old.project.clone()).is_ok()
        && Title::try_from(old.title.clone()).is_ok()
        && Body::try_from(old.content.clone()).is_ok()
        && !old.content.trim().is_empty()
        && old.tags.len() <= 16
        && old
            .tags
            .iter()
            .all(|tag| Tag::try_from(tag.clone()).is_ok())
        && old.sources.len() <= 16
}

// Skips rows that do not fit today's limits rather than failing the upgrade.
fn import(conn: &mut Connection, legacy: Vec<LegacyConcept>) -> Result<usize> {
    let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let mut imported = 0;
    for old in legacy {
        if !fits_todays_limits(&old) {
            tracing::warn!("a legacy concept that does not fit today's limits was not imported");
            continue;
        }
        let concept = Concept {
            id: old.id,
            project: old.project,
            title: old.title,
            content: old.content,
            tags: old.tags,
            sources: old.sources,
            archived: false,
            author: None,
            revision: 1,
            created_at: old.created_at,
            updated_at: old.updated_at,
        };
        let who = Attribution::default();
        if write_new(&tx, &concept, &who, Origin::LegacyBaseline, Op::Baseline)? {
            imported += 1;
        }
    }
    tx.execute(
        "INSERT OR REPLACE INTO meta (key, value) VALUES (?1, ?2)",
        params![LEGACY_IMPORTED, now()],
    )?;
    tx.commit()?;
    Ok(imported)
}

// Read and write happen in one transaction, so a concurrent change is never
// silently overwritten: it is either seen or reported through expected_revision.
fn update(conn: &mut Connection, patch: ConceptPatch) -> Result<Concept> {
    let ConceptPatch {
        id,
        title,
        content,
        tags,
        archived,
        author,
        run,
        expected_revision,
    } = patch;
    if title.is_none() && content.is_none() && tags.is_none() && archived.is_none() {
        return Err(invalid(
            "nothing to update: give at least one field besides id",
        ));
    }
    let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let current = load(&tx, &id.to_string())?.ok_or_else(|| not_found(id))?;
    if let Some(expected) = expected_revision
        && expected != current.revision
    {
        return Err(MemoryError::Conflict(format!(
            "concept {id} is at revision {}, not {expected}; read it again before changing it",
            current.revision
        )));
    }
    let updated = Concept {
        title: title.map_or_else(|| current.title.clone(), String::from),
        content: content.map_or_else(|| current.content.clone(), String::from),
        tags: tags.map_or_else(
            || current.tags.clone(),
            |tags| tag_strings(&tags).into_iter().map(String::from).collect(),
        ),
        archived: archived.unwrap_or(current.archived),
        revision: current.revision + 1,
        updated_at: now(),
        ..current.clone()
    };
    let changed: Vec<&str> = [
        ("title", updated.title != current.title),
        ("content", updated.content != current.content),
        ("tags", updated.tags != current.tags),
        ("archived", updated.archived != current.archived),
    ]
    .into_iter()
    .filter_map(|(field, differs)| differs.then_some(field))
    .collect();
    if changed.is_empty() {
        return Ok(current);
    }

    tx.execute(
        "UPDATE concepts
         SET title = ?2, content = ?3, tags = ?4, archived = ?5, revision = ?6, updated_at = ?7
         WHERE id = ?1",
        params![
            updated.id,
            updated.title,
            updated.content,
            updated.tags.join(" "),
            updated.archived,
            updated.revision,
            updated.updated_at,
        ],
    )?;
    let who = Attribution::new(author, run, None);
    save_revision(&tx, &updated, &who, Origin::Recorded)?;
    let op = match (changed.as_slice(), updated.archived) {
        (["archived"], true) => Op::Archived,
        (["archived"], false) => Op::Restored,
        _ => Op::Updated,
    };
    journal::record(
        &tx,
        Entry::new(&updated.project, RecordType::Concept, &updated.id, op, &who)
            .revision(updated.revision)
            .fields(changed),
    )?;
    tx.commit()?;
    Ok(updated)
}

fn forget(conn: &mut Connection, request: ForgetConcept) -> Result<()> {
    let id = request.id.to_string();
    let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let concept = load(&tx, &id)?.ok_or_else(|| not_found(request.id))?;
    let who = Attribution::new(request.author, request.run, None);
    journal::record(
        &tx,
        Entry::new(&concept.project, RecordType::Concept, &id, Op::Purged, &who),
    )?;
    journal::unindex(&tx, RecordType::Concept, &id)?;
    // Revisions go through ON DELETE CASCADE.
    tx.execute("DELETE FROM concepts WHERE id = ?1", [&id])?;
    tx.commit()?;
    Ok(())
}

fn similar_by_words(conn: &Connection, concept: &Concept) -> Result<Vec<SimilarConcept>> {
    let Some(expression) = words::all_of(&concept.title) else {
        return Ok(Vec::new());
    };
    Ok(conn
        .prepare(
            "SELECT c.id, c.title FROM concepts_fts
             JOIN concepts c ON c.seq = concepts_fts.rowid
             WHERE concepts_fts MATCH ?1 AND c.project = ?2 AND c.id <> ?3 AND c.archived = 0
             ORDER BY bm25(concepts_fts, 4.0, 1.0, 2.0), c.seq DESC
             LIMIT ?4",
        )?
        .query_map(
            params![
                expression,
                concept.project,
                concept.id,
                SIMILAR_LIMIT as i64
            ],
            |row| {
                Ok(SimilarConcept {
                    id: row.get(0)?,
                    title: row.get(1)?,
                    score: SIMILAR_MIN_SCORE,
                })
            },
        )?
        .collect::<rusqlite::Result<_>>()?)
}

#[cfg(test)]
#[path = "tests/concepts.rs"]
mod tests;
