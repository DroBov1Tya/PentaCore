// Search by meaning. Each note keeps its vector beside it in SQLite, so the
// index cannot drift from the records and is purged with them. Notes are
// embedded in the background once a search has loaded the model; without the
// model the memory answers by words.

use anyhow::{Context, anyhow};
use fastembed::{EmbeddingModel, TextEmbedding, TextInitOptions};
use rusqlite::params;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use tokio::sync::OnceCell;

use super::db::Db;
use super::model::{MemoryError, NoteId};

const VECTOR_DIM: usize = 384;
const SYNC_BATCH: usize = 16;
// The model reads about 256 tokens; more text only costs time.
const MAX_EMBEDDED_CHARS: usize = 2000;

type Result<T> = std::result::Result<T, MemoryError>;

#[derive(Debug, Clone, PartialEq)]
pub struct Neighbor {
    pub id: NoteId,
    // Cosine similarity, 1 is identical.
    pub similarity: f32,
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
    // None switches search by meaning off.
    model_cache: Option<PathBuf>,
    embedder: Arc<OnceCell<Arc<Mutex<TextEmbedding>>>>,
    syncing: AtomicBool,
}

fn embedding_text(title: &str, body: &str, tags: &str) -> String {
    let body: String = body.chars().take(MAX_EMBEDDED_CHARS).collect();
    if tags.is_empty() {
        format!("{title}\n{body}")
    } else {
        format!("{title}\n{body}\ntags: {tags}")
    }
}

fn to_blob(vector: &[f32]) -> Vec<u8> {
    vector
        .iter()
        .flat_map(|value| value.to_le_bytes())
        .collect()
}

fn from_blob(blob: &[u8]) -> Option<Vec<f32>> {
    if blob.len() != VECTOR_DIM * 4 {
        return None;
    }
    Some(
        blob.chunks_exact(4)
            .map(|bytes| f32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
            .collect(),
    )
}

// The model's vectors have unit length, so the dot product is the cosine.
pub fn cosine(a: &[f32], b: &[f32]) -> f32 {
    a.iter().zip(b).map(|(x, y)| x * y).sum()
}

impl Semantic {
    pub fn new(db: Db, model_cache: Option<PathBuf>) -> Self {
        Self {
            inner: Arc::new(Inner {
                db,
                model_cache,
                embedder: Arc::new(OnceCell::new()),
                syncing: AtomicBool::new(false),
            }),
        }
    }

    // Notes closest in meaning to the text. Fails when the model is unavailable.
    pub async fn search(
        &self,
        text: &str,
        project: Option<&str>,
        limit: usize,
    ) -> Result<Vec<Neighbor>> {
        let query = self
            .inner
            .embed(vec![text.to_string()])
            .await?
            .into_iter()
            .next()
            .context("embedder returned no vector")?;
        self.catch_up();
        let project = project.map(String::from);
        self.inner
            .db
            .run(move |conn| {
                let mut neighbors: Vec<Neighbor> = conn
                    .prepare_cached(
                        "SELECT id, embedding FROM notes
                         WHERE embedding IS NOT NULL AND (?1 IS NULL OR project = ?1)",
                    )?
                    .query_map([project], |row| {
                        let blob: Vec<u8> = row.get(1)?;
                        Ok((row.get::<_, NoteId>(0)?, blob))
                    })?
                    .filter_map(|row| match row {
                        Ok((id, blob)) => from_blob(&blob).map(|vector| {
                            Ok(Neighbor {
                                id,
                                similarity: cosine(&query, &vector),
                            })
                        }),
                        Err(error) => Some(Err(error)),
                    })
                    .collect::<rusqlite::Result<_>>()?;
                neighbors.sort_by(|a, b| b.similarity.total_cmp(&a.similarity));
                neighbors.truncate(limit);
                Ok(neighbors)
            })
            .await
    }

    // Vectors of the given notes, for those that have one.
    pub async fn vectors(&self, ids: Vec<NoteId>) -> Result<HashMap<NoteId, Vec<f32>>> {
        self.inner
            .db
            .run(move |conn| {
                let wanted = serde_json::Value::from(ids).to_string();
                let found = conn
                    .prepare_cached(
                        "SELECT id, embedding FROM notes
                         WHERE embedding IS NOT NULL
                           AND id IN (SELECT value FROM json_each(?1))",
                    )?
                    .query_map([wanted], |row| {
                        let blob: Vec<u8> = row.get(1)?;
                        Ok((row.get::<_, NoteId>(0)?, blob))
                    })?
                    .filter_map(|row| match row {
                        Ok((id, blob)) => from_blob(&blob).map(|vector| Ok((id, vector))),
                        Err(error) => Some(Err(error)),
                    })
                    .collect::<rusqlite::Result<_>>()?;
                Ok(found)
            })
            .await
    }

    // Embeds pending notes in the background. A no-op until a search has
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
                tracing::warn!("search by meaning is behind: {error}");
            }
        });
    }

    // Notes whose current revision has no vector yet.
    pub async fn pending(&self) -> Result<i64> {
        self.inner
            .db
            .run(|conn| {
                Ok(conn.query_row(
                    "SELECT count(*) FROM notes WHERE embedded_revision IS NOT revision",
                    [],
                    |row| row.get(0),
                )?)
            })
            .await
    }
}

impl Inner {
    async fn sync_all(&self) -> Result<()> {
        loop {
            let batch: Vec<(NoteId, i64, String)> = self
                .db
                .run(|conn| {
                    Ok(conn
                        .prepare_cached(
                            "SELECT id, revision, title, body, tags FROM notes
                             WHERE embedded_revision IS NOT revision ORDER BY id DESC LIMIT ?1",
                        )?
                        .query_map([SYNC_BATCH as i64], |row| {
                            let (title, body, tags): (String, String, String) =
                                (row.get(2)?, row.get(3)?, row.get(4)?);
                            Ok((
                                row.get(0)?,
                                row.get(1)?,
                                embedding_text(&title, &body, &tags),
                            ))
                        })?
                        .collect::<rusqlite::Result<_>>()?)
                })
                .await?;
            if batch.is_empty() {
                return Ok(());
            }
            let texts = batch.iter().map(|(_, _, text)| text.clone()).collect();
            let vectors = self.embed(texts).await?;
            self.db
                .run(move |conn| {
                    let tx = conn.transaction()?;
                    for ((id, revision, _), vector) in batch.into_iter().zip(vectors) {
                        // A note changed meanwhile stays pending; a purged one is gone.
                        tx.execute(
                            "UPDATE notes SET embedding = ?3, embedded_revision = ?2
                             WHERE id = ?1 AND revision = ?2",
                            params![id, revision, to_blob(&vector)],
                        )?;
                    }
                    tx.commit()?;
                    Ok(())
                })
                .await?;
        }
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
        let well_formed =
            vectors.len() == expected && vectors.iter().all(|vector| vector.len() == VECTOR_DIM);
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
