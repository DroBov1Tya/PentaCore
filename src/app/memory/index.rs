// Search by meaning. LanceDB holds ids and vectors only and is rebuilt from
// SQLite. Records are embedded in the background once a search has loaded
// the model; without the model the memory answers by words.

use anyhow::{Context, anyhow};
use arrow_array::{
    Array, ArrayRef, FixedSizeListArray, Float32Array, Int64Array, RecordBatch,
    RecordBatchIterator, RecordBatchReader, StringArray,
};
use arrow_schema::{DataType, Field, Schema};
use fastembed::{EmbeddingModel, TextEmbedding, TextInitOptions};
use futures::TryStreamExt;
use lancedb::query::{ExecutableQuery, QueryBase, Select};
use lancedb::{Connection, DistanceType};
use rusqlite::params;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use tokio::sync::OnceCell;

use super::db::Db;
use super::journal::RecordType;
use super::model::MemoryError;

const TABLE: &str = "memory_index";
// Concepts table of earlier versions. Read once; only purges touch it.
const LEGACY_TABLE: &str = "concepts";
const VECTOR_DIM: i32 = 384;
const DISTANCE_COLUMN: &str = "_distance";
const SYNC_BATCH: usize = 16;
const DELETION_BATCH: usize = 256;
// The model reads about 256 tokens; more text only costs time.
const MAX_EMBEDDED_CHARS: usize = 2000;

type Result<T> = std::result::Result<T, MemoryError>;

#[derive(Debug, Clone, PartialEq)]
pub struct Neighbor {
    pub record: RecordType,
    pub id: String,
    // Cosine similarity, 1 is identical.
    pub similarity: f32,
}

struct Pending {
    record: RecordType,
    id: String,
    project: String,
    revision: i64,
    text: String,
}

pub struct LegacyConcept {
    pub id: String,
    pub project: String,
    pub title: String,
    pub content: String,
    pub tags: Vec<String>,
    pub sources: Vec<i64>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Clone)]
pub struct Semantic {
    inner: Arc<Inner>,
}

struct Running<'a>(&'a AtomicBool);

impl Drop for Running<'_> {
    fn drop(&mut self) {
        self.0.store(false, Ordering::Release);
    }
}

struct Inner {
    db: Db,
    index_path: PathBuf,
    // None switches search by meaning off; the memory then runs on words.
    model_cache: Option<PathBuf>,
    lance: OnceCell<Connection>,
    embedder: Arc<OnceCell<Arc<Mutex<TextEmbedding>>>>,
    syncing: AtomicBool,
}

pub fn embedding_text(title: &str, body: &str, tags: &[String]) -> String {
    let body: String = body.chars().take(MAX_EMBEDDED_CHARS).collect();
    if tags.is_empty() {
        format!("{title}\n{body}")
    } else {
        format!("{title}\n{body}\ntags: {}", tags.join(", "))
    }
}

impl Semantic {
    pub fn new(db: Db, index_path: PathBuf, model_cache: Option<PathBuf>) -> Self {
        Self {
            inner: Arc::new(Inner {
                db,
                index_path,
                model_cache,
                lance: OnceCell::new(),
                embedder: Arc::new(OnceCell::new()),
                syncing: AtomicBool::new(false),
            }),
        }
    }

    // Records closest in meaning to the text. Fails when the model is unavailable.
    pub async fn search(
        &self,
        text: &str,
        project: Option<&str>,
        only: Option<RecordType>,
        limit: usize,
    ) -> Result<Vec<Neighbor>> {
        let vector = self.inner.embed_one(text.to_string()).await?;
        self.catch_up();
        self.inner.nearest(&vector, project, only, limit).await
    }

    // Embeds one record right away, so it can be compared with the others.
    pub async fn index_now(
        &self,
        record: RecordType,
        id: &str,
        project: &str,
        revision: i64,
        text: String,
    ) -> Result<Vec<f32>> {
        let vector = self.inner.embed_one(text).await?;
        let pending = Pending {
            record,
            id: id.to_string(),
            project: project.to_string(),
            revision,
            text: String::new(),
        };
        self.inner.store(vec![(pending, vector.clone())]).await?;
        self.inner
            .drop_if_purged(vec![(record, id.to_string())])
            .await?;
        Ok(vector)
    }

    pub async fn nearest(
        &self,
        vector: &[f32],
        project: Option<&str>,
        only: Option<RecordType>,
        limit: usize,
    ) -> Result<Vec<Neighbor>> {
        self.inner.nearest(vector, project, only, limit).await
    }

    // Vectors of the given records, for those that have one.
    pub async fn vectors(
        &self,
        of: &[(RecordType, String)],
    ) -> Result<HashMap<(RecordType, String), Vec<f32>>> {
        self.inner.vectors(of).await
    }

    // Indexes pending records in the background. A no-op until a search has
    // loaded the model, so a write never starts a download.
    pub fn catch_up(&self) {
        if self.inner.embedder.get().is_none() || self.inner.syncing.swap(true, Ordering::AcqRel) {
            return;
        }
        let inner = Arc::clone(&self.inner);
        tokio::spawn(async move {
            // Cleared on every exit, a panic included.
            let _running = Running(&inner.syncing);
            if let Err(error) = inner.sync_all().await {
                tracing::warn!("search index is behind: {error}");
            }
        });
    }

    // Brings the index fully up to date. Loads the model if needed.
    #[cfg(test)]
    pub async fn sync(&self) -> Result<()> {
        self.inner.sync_all().await
    }

    // Removes the vectors of purged records. Needs no model.
    pub async fn drop_purged(&self) -> Result<()> {
        self.inner.drop_purged().await
    }

    // Records whose current revision has no vector yet.
    pub async fn pending(&self) -> Result<i64> {
        self.inner
            .db
            .run(|conn| {
                Ok(conn.query_row(
                    "SELECT (SELECT count(*) FROM notes WHERE indexed_revision IS NOT revision)
                          + (SELECT count(*) FROM concepts WHERE indexed_revision IS NOT revision)",
                    [],
                    |row| row.get(0),
                )?)
            })
            .await
    }

    // Concepts kept by earlier versions, or None when there never were any.
    pub async fn legacy_concepts(&self) -> Result<Option<Vec<LegacyConcept>>> {
        self.inner.legacy_concepts().await
    }
}

impl Inner {
    async fn lance(&self) -> Result<&Connection> {
        self.lance.get_or_try_init(|| self.connect()).await
    }

    async fn connect(&self) -> Result<Connection> {
        let uri = self
            .index_path
            .to_str()
            .context("search index path is not valid UTF-8")?;
        let lance = lancedb::connect(uri).execute().await?;
        if !has_table(&lance, TABLE).await? {
            match lance.create_empty_table(TABLE, schema()).execute().await {
                // Another process sharing this directory created it first.
                Ok(_) | Err(lancedb::Error::TableAlreadyExists { .. }) => {}
                Err(error) => return Err(error.into()),
            }
            // An empty index holds nothing, whatever the records claim.
            self.db
                .run(|conn| {
                    conn.execute_batch(
                        "UPDATE notes SET indexed_revision = NULL WHERE indexed_revision IS NOT NULL;
                         UPDATE concepts SET indexed_revision = NULL WHERE indexed_revision IS NOT NULL;",
                    )?;
                    Ok(())
                })
                .await?;
        }
        Ok(lance)
    }

    async fn table(&self) -> Result<lancedb::Table> {
        Ok(self.lance().await?.open_table(TABLE).execute().await?)
    }

    async fn nearest(
        &self,
        vector: &[f32],
        project: Option<&str>,
        only: Option<RecordType>,
        limit: usize,
    ) -> Result<Vec<Neighbor>> {
        let mut search = self
            .table()
            .await?
            .query()
            .nearest_to(vector)?
            .distance_type(DistanceType::Cosine)
            .limit(limit)
            .select(Select::columns(&["kind", "id", DISTANCE_COLUMN]));
        let mut filters = Vec::new();
        if let Some(project) = project {
            filters.push(format!("project = {}", sql_literal(project)));
        }
        if let Some(record) = only {
            filters.push(format!("kind = {}", sql_literal(record.as_str())));
        }
        if !filters.is_empty() {
            search = search.only_if(filters.join(" AND "));
        }
        let batches: Vec<RecordBatch> = search.execute().await?.try_collect().await?;

        let mut neighbors = Vec::new();
        for batch in &batches {
            let kinds = text_column(batch, "kind")?;
            let ids = text_column(batch, "id")?;
            let distances = batch
                .column_by_name(DISTANCE_COLUMN)
                .and_then(|column| column.as_any().downcast_ref::<Float32Array>())
                .context("vector search returned no _distance column")?;
            for row in 0..batch.num_rows() {
                let Some(record) = RecordType::parse(kinds.value(row)) else {
                    continue;
                };
                neighbors.push(Neighbor {
                    record,
                    id: ids.value(row).to_string(),
                    similarity: 1.0 - distances.value(row),
                });
            }
        }
        Ok(neighbors)
    }

    async fn vectors(
        &self,
        of: &[(RecordType, String)],
    ) -> Result<HashMap<(RecordType, String), Vec<f32>>> {
        let mut found = HashMap::new();
        for record in [RecordType::Note, RecordType::Concept] {
            let ids: Vec<String> = of
                .iter()
                .filter(|(kind, _)| *kind == record)
                .map(|(_, id)| sql_literal(id))
                .collect();
            if ids.is_empty() {
                continue;
            }
            let filter = format!(
                "kind = {} AND id IN ({})",
                sql_literal(record.as_str()),
                ids.join(", ")
            );
            let batches: Vec<RecordBatch> = self
                .table()
                .await?
                .query()
                .only_if(filter)
                .limit(ids.len())
                .select(Select::columns(&["id", "vector"]))
                .execute()
                .await?
                .try_collect()
                .await?;
            for batch in &batches {
                let ids = text_column(batch, "id")?;
                let vectors = batch
                    .column_by_name("vector")
                    .and_then(|column| column.as_any().downcast_ref::<FixedSizeListArray>())
                    .context("the index has no vector column")?;
                for row in 0..batch.num_rows() {
                    let values = vectors.value(row);
                    let values = values
                        .as_any()
                        .downcast_ref::<Float32Array>()
                        .context("the index holds vectors of an unexpected type")?;
                    found.insert(
                        (record, ids.value(row).to_string()),
                        values.values().to_vec(),
                    );
                }
            }
        }
        Ok(found)
    }

    async fn sync_all(&self) -> Result<()> {
        self.drop_purged().await?;
        loop {
            let batch = self.pending_batch().await?;
            if batch.is_empty() {
                return Ok(());
            }
            let texts = batch.iter().map(|pending| pending.text.clone()).collect();
            let stored: Vec<(RecordType, String)> = batch
                .iter()
                .map(|pending| (pending.record, pending.id.clone()))
                .collect();
            let vectors = self.embed(texts).await?;
            self.store(batch.into_iter().zip(vectors).collect()).await?;
            self.drop_if_purged(stored).await?;
        }
    }

    // Re-queues the vector of a record purged while it was being embedded.
    async fn drop_if_purged(&self, stored: Vec<(RecordType, String)>) -> Result<()> {
        let queued = self
            .db
            .run(move |conn| {
                let tx = conn.transaction()?;
                let mut queued = 0usize;
                for (record, id) in stored {
                    let sql = match record {
                        RecordType::Concept => {
                            "SELECT EXISTS (SELECT 1 FROM concepts WHERE id = ?1)"
                        }
                        _ => "SELECT EXISTS (SELECT 1 FROM notes WHERE id = CAST(?1 AS INTEGER))",
                    };
                    let exists: bool = tx.query_row(sql, [&id], |row| row.get(0))?;
                    if !exists {
                        super::journal::unindex(&tx, record, &id)?;
                        queued += 1;
                    }
                }
                tx.commit()?;
                Ok(queued)
            })
            .await?;
        if queued > 0 {
            self.drop_purged().await?;
        }
        Ok(())
    }

    async fn pending_batch(&self) -> Result<Vec<Pending>> {
        self.db
            .run(|conn| {
                let mut batch: Vec<Pending> = conn
                    .prepare(
                        "SELECT id, project, revision, title, content, tags FROM concepts
                         WHERE indexed_revision IS NOT revision ORDER BY seq LIMIT ?1",
                    )?
                    .query_map([SYNC_BATCH as i64], |row| {
                        pending_from_row(RecordType::Concept, row)
                    })?
                    .collect::<rusqlite::Result<_>>()?;
                let room = SYNC_BATCH - batch.len();
                if room > 0 {
                    let notes: Vec<Pending> = conn
                        .prepare(
                            "SELECT CAST(id AS TEXT), project, revision, title, body, tags FROM notes
                             WHERE indexed_revision IS NOT revision ORDER BY id DESC LIMIT ?1",
                        )?
                        .query_map([room as i64], |row| pending_from_row(RecordType::Note, row))?
                        .collect::<rusqlite::Result<_>>()?;
                    batch.extend(notes);
                }
                Ok(batch)
            })
            .await
    }

    // An older vector never replaces a newer one, whichever writer is slower.
    async fn store(&self, rows: Vec<(Pending, Vec<f32>)>) -> Result<()> {
        if rows.is_empty() {
            return Ok(());
        }
        let table = self.table().await?;
        let mut merge = table.merge_insert(&["kind", "id"]);
        merge
            .when_matched_update_all(Some("target.revision <= source.revision".to_string()))
            .when_not_matched_insert_all();
        merge.execute(index_rows(&rows)?).await?;

        let done: Vec<(RecordType, String, i64)> = rows
            .into_iter()
            .map(|(pending, _)| (pending.record, pending.id, pending.revision))
            .collect();
        self.db
            .run(move |conn| {
                let tx = conn.transaction()?;
                for (record, id, revision) in done {
                    // A record changed in the meantime stays pending.
                    let sql = match record {
                        RecordType::Concept => {
                            "UPDATE concepts SET indexed_revision = ?2 WHERE id = ?1 AND revision = ?2"
                        }
                        _ => "UPDATE notes SET indexed_revision = ?2
                              WHERE id = CAST(?1 AS INTEGER) AND revision = ?2",
                    };
                    tx.execute(sql, params![id, revision])?;
                }
                tx.commit()?;
                Ok(())
            })
            .await
    }

    async fn drop_purged(&self) -> Result<()> {
        loop {
            let doomed: Vec<(RecordType, String)> = self
                .db
                .run(|conn| {
                    Ok(conn
                        .prepare("SELECT record_type, record_id FROM index_deletions LIMIT ?1")?
                        .query_map([DELETION_BATCH as i64], |row| {
                            Ok((row.get(0)?, row.get(1)?))
                        })?
                        .collect::<rusqlite::Result<_>>()?)
                })
                .await?;
            if doomed.is_empty() {
                return Ok(());
            }
            // Without an index directory nothing was ever indexed.
            if self.index_path.exists() {
                self.delete_vectors(&doomed).await?;
            }
            self.db
                .run(move |conn| {
                    let tx = conn.transaction()?;
                    for (record, id) in doomed {
                        tx.execute(
                            "DELETE FROM index_deletions WHERE record_type = ?1 AND record_id = ?2",
                            params![record, id],
                        )?;
                    }
                    tx.commit()?;
                    Ok(())
                })
                .await?;
        }
    }

    async fn delete_vectors(&self, doomed: &[(RecordType, String)]) -> Result<()> {
        let ids_of = |wanted: RecordType| -> Vec<String> {
            doomed
                .iter()
                .filter(|(record, _)| *record == wanted)
                .map(|(_, id)| sql_literal(id))
                .collect()
        };
        let table = self.table().await?;
        for record in [RecordType::Note, RecordType::Concept] {
            let ids = ids_of(record);
            if !ids.is_empty() {
                let filter = format!(
                    "kind = {} AND id IN ({})",
                    sql_literal(record.as_str()),
                    ids.join(", ")
                );
                table.delete(&filter).await?;
            }
        }
        let concepts = ids_of(RecordType::Concept);
        let lance = self.lance().await?;
        if !concepts.is_empty() && has_table(lance, LEGACY_TABLE).await? {
            let legacy = lance.open_table(LEGACY_TABLE).execute().await?;
            legacy
                .delete(&format!("id IN ({})", concepts.join(", ")))
                .await?;
        }
        Ok(())
    }

    async fn legacy_concepts(&self) -> Result<Option<Vec<LegacyConcept>>> {
        if !self.index_path.exists() {
            return Ok(None);
        }
        let lance = self.lance().await?;
        if !has_table(lance, LEGACY_TABLE).await? {
            return Ok(None);
        }
        let batches: Vec<RecordBatch> = lance
            .open_table(LEGACY_TABLE)
            .execute()
            .await?
            .query()
            .select(Select::columns(&[
                "id",
                "project",
                "title",
                "content",
                "tags",
                "sources",
                "created_at",
                "updated_at",
            ]))
            .execute()
            .await?
            .try_collect()
            .await?;
        let mut concepts = Vec::new();
        for batch in &batches {
            let column = |name| text_column(batch, name);
            let (id, project, title, content) = (
                column("id")?,
                column("project")?,
                column("title")?,
                column("content")?,
            );
            let (tags, sources, created_at, updated_at) = (
                column("tags")?,
                column("sources")?,
                column("created_at")?,
                column("updated_at")?,
            );
            for row in 0..batch.num_rows() {
                // One unreadable row must not hold back the rest.
                let (Ok(tags), Ok(sources)) = (
                    serde_json::from_str::<Vec<String>>(tags.value(row)),
                    serde_json::from_str::<Vec<i64>>(sources.value(row)),
                ) else {
                    tracing::warn!("a legacy concept with unreadable tags or sources was skipped");
                    continue;
                };
                concepts.push(LegacyConcept {
                    id: id.value(row).to_string(),
                    project: project.value(row).to_string(),
                    title: title.value(row).to_string(),
                    content: content.value(row).to_string(),
                    tags,
                    sources,
                    created_at: created_at.value(row).to_string(),
                    updated_at: updated_at.value(row).to_string(),
                });
            }
        }
        Ok(Some(concepts))
    }

    async fn embed_one(&self, text: String) -> Result<Vec<f32>> {
        self.embed(vec![text])
            .await?
            .into_iter()
            .next()
            .context("embedder returned no vector")
            .map_err(Into::into)
    }

    // CPU bound, so it runs on the blocking pool.
    async fn embed(&self, texts: Vec<String>) -> Result<Vec<Vec<f32>>> {
        let expected = texts.len();
        let embedder = self.embedder().await?;
        let vectors = tokio::task::spawn_blocking(move || {
            let mut model = embedder
                .lock()
                .map_err(|_| anyhow!("embedder mutex is poisoned"))?;
            model
                .embed(texts, None)
                .map_err(|e| anyhow!("embedding failed: {e}"))
        })
        .await??;

        let well_formed = vectors.len() == expected
            && vectors
                .iter()
                .all(|vector| vector.len() == VECTOR_DIM as usize);
        if !well_formed {
            return Err(anyhow!("embedder returned vectors of an unexpected shape").into());
        }
        Ok(vectors)
    }

    // Loaded in its own task, so a cancelled caller does not restart the download.
    async fn embedder(&self) -> Result<Arc<Mutex<TextEmbedding>>> {
        if let Some(embedder) = self.embedder.get() {
            return Ok(Arc::clone(embedder));
        }
        let cell = Arc::clone(&self.embedder);
        let cache = self
            .model_cache
            .clone()
            .context("search by meaning is switched off")?;
        tokio::spawn(async move {
            cell.get_or_try_init(|| load_embedder(cache))
                .await
                .map(Arc::clone)
        })
        .await?
    }
}

async fn load_embedder(cache: PathBuf) -> Result<Arc<Mutex<TextEmbedding>>> {
    tracing::info!("loading embedding model (downloaded on first use)");
    let model = tokio::task::spawn_blocking(move || {
        let options = TextInitOptions::new(EmbeddingModel::AllMiniLML6V2)
            .with_cache_dir(cache)
            .with_show_download_progress(false);
        TextEmbedding::try_new(options).map_err(|e| anyhow!("failed to load embedding model: {e}"))
    })
    .await??;
    Ok(Arc::new(Mutex::new(model)))
}

async fn has_table(lance: &Connection, name: &str) -> Result<bool> {
    Ok(lance
        .table_names()
        .execute()
        .await?
        .iter()
        .any(|table| table == name))
}

fn pending_from_row(record: RecordType, row: &rusqlite::Row<'_>) -> rusqlite::Result<Pending> {
    let (title, body, tags): (String, String, String) = (row.get(3)?, row.get(4)?, row.get(5)?);
    let tags: Vec<String> = tags.split_whitespace().map(String::from).collect();
    Ok(Pending {
        record,
        id: row.get(0)?,
        project: row.get(1)?,
        revision: row.get(2)?,
        text: embedding_text(&title, &body, &tags),
    })
}

// Only ids, project names and record kinds get here, each from an allowlist.
fn sql_literal(value: &str) -> String {
    format!("'{}'", value.replace('\'', "''"))
}

fn schema() -> Arc<Schema> {
    Arc::new(Schema::new(vec![
        Field::new("kind", DataType::Utf8, false),
        Field::new("id", DataType::Utf8, false),
        Field::new("project", DataType::Utf8, false),
        Field::new("revision", DataType::Int64, false),
        Field::new(
            "vector",
            DataType::FixedSizeList(vector_item(), VECTOR_DIM),
            false,
        ),
    ]))
}

fn vector_item() -> Arc<Field> {
    Arc::new(Field::new("item", DataType::Float32, true))
}

fn index_rows(rows: &[(Pending, Vec<f32>)]) -> Result<Box<dyn RecordBatchReader + Send>> {
    let text = |value: fn(&Pending) -> &str| -> ArrayRef {
        Arc::new(StringArray::from_iter_values(
            rows.iter().map(|(pending, _)| value(pending)),
        ))
    };
    let revisions: ArrayRef = Arc::new(Int64Array::from_iter_values(
        rows.iter().map(|(pending, _)| pending.revision),
    ));
    let flat: Vec<f32> = rows
        .iter()
        .flat_map(|(_, vector)| vector.iter().copied())
        .collect();
    let vectors = FixedSizeListArray::try_new(
        vector_item(),
        VECTOR_DIM,
        Arc::new(Float32Array::from(flat)),
        None,
    )?;
    let batch = RecordBatch::try_new(
        schema(),
        vec![
            text(|pending| pending.record.as_str()),
            text(|pending| &pending.id),
            text(|pending| &pending.project),
            revisions,
            Arc::new(vectors),
        ],
    )?;
    Ok(Box::new(RecordBatchIterator::new(
        vec![Ok(batch)],
        schema(),
    )))
}

fn text_column<'a>(batch: &'a RecordBatch, name: &str) -> Result<&'a StringArray> {
    batch
        .column_by_name(name)
        .and_then(|column| column.as_any().downcast_ref::<StringArray>())
        .ok_or_else(|| anyhow!("the index has no text column '{name}'").into())
}

#[cfg(test)]
#[path = "tests/index.rs"]
mod tests;
