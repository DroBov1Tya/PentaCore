use anyhow::{Context, anyhow};
use arrow_array::{
    Array, ArrayRef, FixedSizeListArray, Float32Array, RecordBatch, RecordBatchIterator,
    RecordBatchReader, StringArray,
};
use arrow_schema::{DataType, Field, Schema};
use chrono::{SecondsFormat, Utc};
use fastembed::{EmbeddingModel, TextEmbedding, TextInitOptions};
use futures::TryStreamExt;
use lancedb::Connection;
use lancedb::query::{ExecutableQuery, QueryBase, Select};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use tokio::sync::OnceCell;
use uuid::Uuid;

use super::model::{
    AtMost, Body, Limit, MemoryError, NoteId, ProjectName, Scope, SearchText, Tags, Title, invalid,
    tag_strings,
};

const TABLE: &str = "concepts";
const VECTOR_DIM: i32 = 384;
const DEFAULT_SEARCH_LIMIT: usize = 5;
const DISTANCE_COLUMN: &str = "_distance";
const SIMILAR_LIMIT: usize = 3;
// Tuned by eye on paraphrases. A hint for the caller, not a verdict.
const SIMILAR_MIN_SCORE: f32 = 0.6;
const TEXT_COLUMNS: [&str; 8] = [
    "id",
    "project",
    "title",
    "content",
    "tags",
    "sources",
    "created_at",
    "updated_at",
];

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
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConceptPatch {
    pub id: ConceptId,
    pub title: Option<Title>,
    pub content: Option<Body>,
    pub tags: Option<Tags>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConceptQuery {
    pub query: SearchText,
    pub project: Option<Scope>,
    pub limit: Option<Limit>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Concept {
    pub id: String,
    pub project: String,
    pub title: String,
    pub content: String,
    pub tags: Vec<String>,
    pub sources: Vec<NoteId>,
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

#[derive(Debug, Serialize)]
pub struct ConceptHit {
    #[serde(flatten)]
    pub concept: Concept,
    pub score: f32,
}

// Database and model open lazily, so startup stays instant.
pub struct ConceptStore {
    db_path: PathBuf,
    model_cache: PathBuf,
    db: OnceCell<Connection>,
    embedder: Arc<OnceCell<Arc<Mutex<TextEmbedding>>>>,
}

impl ConceptStore {
    pub fn new(db_path: PathBuf, model_cache: PathBuf) -> Self {
        Self {
            db_path,
            model_cache,
            db: OnceCell::new(),
            embedder: Arc::new(OnceCell::new()),
        }
    }

    pub async fn memorize(
        &self,
        input: NewConcept,
        default_project: ProjectName,
    ) -> Result<MemorizedConcept> {
        require_content(&input.content)?;
        let now = now();
        let concept = Concept {
            id: Uuid::new_v4().to_string(),
            project: input.project.unwrap_or(default_project).into(),
            title: input.title.into(),
            content: input.content.into(),
            tags: tag_strings(&input.tags)
                .into_iter()
                .map(String::from)
                .collect(),
            sources: input.sources.as_slice().to_vec(),
            created_at: now.clone(),
            updated_at: now,
        };
        let vector = self.embed(embedding_text(&concept)).await?;

        let similar = self
            .nearest(&vector, Some(&concept.project), SIMILAR_LIMIT)
            .await?
            .into_iter()
            .filter(|hit| hit.score >= SIMILAR_MIN_SCORE)
            .map(|hit| SimilarConcept {
                id: hit.concept.id,
                title: hit.concept.title,
                score: hit.score,
            })
            .collect();
        self.table()
            .await?
            .add(rows_of(&concept, vector)?)
            .execute()
            .await?;
        Ok(MemorizedConcept { concept, similar })
    }

    pub async fn search(
        &self,
        query: ConceptQuery,
        default_scope: Scope,
    ) -> Result<Vec<ConceptHit>> {
        let vector = self.embed(query.query.as_str().to_string()).await?;
        let scope = query.project.unwrap_or(default_scope);
        self.nearest(
            &vector,
            scope.project(),
            Limit::or(query.limit, DEFAULT_SEARCH_LIMIT),
        )
        .await
    }

    async fn nearest(
        &self,
        vector: &[f32],
        project: Option<&str>,
        limit: usize,
    ) -> Result<Vec<ConceptHit>> {
        let mut columns = TEXT_COLUMNS.to_vec();
        columns.push(DISTANCE_COLUMN);
        let mut search = self
            .table()
            .await?
            .query()
            .nearest_to(vector)?
            .limit(limit)
            .select(Select::columns(&columns));
        if let Some(project) = project {
            search = search.only_if(format!("project = {}", sql_literal(project)));
        }
        let batches: Vec<RecordBatch> = search.execute().await?.try_collect().await?;

        let mut hits = Vec::new();
        for batch in &batches {
            let distances = batch
                .column_by_name(DISTANCE_COLUMN)
                .and_then(|column| column.as_any().downcast_ref::<Float32Array>())
                .context("vector search returned no _distance column")?;
            for (row, concept) in concepts_from_batch(batch)?.into_iter().enumerate() {
                hits.push(ConceptHit {
                    concept,
                    score: 1.0 / (1.0 + distances.value(row)),
                });
            }
        }
        Ok(hits)
    }

    pub async fn update(&self, patch: ConceptPatch) -> Result<Concept> {
        let ConceptPatch {
            id,
            title,
            content,
            tags,
        } = patch;
        if title.is_none() && content.is_none() && tags.is_none() {
            return Err(invalid(
                "nothing to update: give at least one field besides id",
            ));
        }
        if let Some(content) = &content {
            require_content(content)?;
        }

        let current = self.get(id).await?;
        let updated = Concept {
            title: title.map_or(current.title, String::from),
            content: content.map_or(current.content, String::from),
            tags: tags.map_or(current.tags, |tags| {
                tag_strings(&tags).into_iter().map(String::from).collect()
            }),
            updated_at: now(),
            ..current
        };

        let vector = self.embed(embedding_text(&updated)).await?;
        let table = self.table().await?;
        let mut merge = table.merge_insert(&["id"]);
        merge.when_matched_update_all(None);
        let merged = merge.execute(rows_of(&updated, vector)?).await?;
        // Deleted by someone else between the read and this write.
        if merged.num_updated_rows == 0 {
            return Err(not_found(id));
        }
        Ok(updated)
    }

    pub async fn forget(&self, id: ConceptId) -> Result<()> {
        let deleted = self.table().await?.delete(&id_filter(id)).await?;
        if deleted.num_deleted_rows == 0 {
            return Err(not_found(id));
        }
        Ok(())
    }

    async fn get(&self, id: ConceptId) -> Result<Concept> {
        let batches: Vec<RecordBatch> = self
            .table()
            .await?
            .query()
            .only_if(id_filter(id))
            .limit(1)
            .select(Select::columns(&TEXT_COLUMNS))
            .execute()
            .await?
            .try_collect()
            .await?;
        for batch in &batches {
            if let Some(concept) = concepts_from_batch(batch)?.into_iter().next() {
                return Ok(concept);
            }
        }
        Err(not_found(id))
    }

    async fn table(&self) -> Result<lancedb::Table> {
        let db = self.db.get_or_try_init(|| self.connect()).await?;
        Ok(db.open_table(TABLE).execute().await?)
    }

    async fn connect(&self) -> Result<Connection> {
        let uri = self
            .db_path
            .to_str()
            .context("concept store path is not valid UTF-8")?;
        let db = lancedb::connect(uri).execute().await?;
        let exists = db
            .table_names()
            .execute()
            .await?
            .iter()
            .any(|name| name == TABLE);
        if !exists {
            match db.create_empty_table(TABLE, schema()).execute().await {
                // Another process sharing this directory created it first.
                Ok(_) | Err(lancedb::Error::TableAlreadyExists { .. }) => {}
                Err(error) => return Err(error.into()),
            }
        }
        Ok(db)
    }

    // CPU bound, so it runs on the blocking pool.
    async fn embed(&self, text: String) -> Result<Vec<f32>> {
        let embedder = self.embedder().await?;
        let vectors = tokio::task::spawn_blocking(move || {
            let mut model = embedder
                .lock()
                .map_err(|_| anyhow!("embedder mutex is poisoned"))?;
            model
                .embed(vec![text], None)
                .map_err(|e| anyhow!("embedding failed: {e}"))
        })
        .await??;

        let vector = vectors
            .into_iter()
            .next()
            .context("embedder returned no vector")?;
        if vector.len() != VECTOR_DIM as usize {
            return Err(anyhow!(
                "embedder returned {} dimensions, expected {VECTOR_DIM}",
                vector.len()
            )
            .into());
        }
        Ok(vector)
    }

    // Loaded in its own task, so a cancelled caller does not restart the download.
    async fn embedder(&self) -> Result<Arc<Mutex<TextEmbedding>>> {
        if let Some(embedder) = self.embedder.get() {
            return Ok(Arc::clone(embedder));
        }
        let cell = Arc::clone(&self.embedder);
        let cache = self.model_cache.clone();
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

fn rows_of(concept: &Concept, vector: Vec<f32>) -> Result<Box<dyn RecordBatchReader + Send>> {
    let batch = batch_of(concept, vector)?;
    Ok(Box::new(RecordBatchIterator::new(
        vec![Ok(batch)],
        schema(),
    )))
}

fn now() -> String {
    Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true)
}

fn not_found(id: ConceptId) -> MemoryError {
    MemoryError::NotFound(format!("concept {}", id.0))
}

fn require_content(content: &Body) -> Result<()> {
    if content.as_str().trim().is_empty() {
        return Err(invalid("content must not be empty"));
    }
    Ok(())
}

fn sql_literal(value: &str) -> String {
    format!("'{}'", value.replace('\'', "''"))
}

fn id_filter(id: ConceptId) -> String {
    format!("id = {}", sql_literal(&id.0.to_string()))
}

fn embedding_text(concept: &Concept) -> String {
    if concept.tags.is_empty() {
        format!("{}\n{}", concept.title, concept.content)
    } else {
        format!(
            "{}\n{}\ntags: {}",
            concept.title,
            concept.content,
            concept.tags.join(", ")
        )
    }
}

fn schema() -> Arc<Schema> {
    let mut fields: Vec<Field> = TEXT_COLUMNS
        .iter()
        .map(|name| Field::new(*name, DataType::Utf8, false))
        .collect();
    fields.push(Field::new(
        "vector",
        DataType::FixedSizeList(vector_item(), VECTOR_DIM),
        false,
    ));
    Arc::new(Schema::new(fields))
}

fn vector_item() -> Arc<Field> {
    Arc::new(Field::new("item", DataType::Float32, true))
}

fn batch_of(concept: &Concept, vector: Vec<f32>) -> Result<RecordBatch> {
    let text = |value: &str| -> ArrayRef { Arc::new(StringArray::from(vec![value])) };
    let tags = serde_json::to_string(&concept.tags).context("failed to encode tags")?;
    let sources = serde_json::to_string(&concept.sources).context("failed to encode sources")?;
    let vector = FixedSizeListArray::try_new(
        vector_item(),
        VECTOR_DIM,
        Arc::new(Float32Array::from(vector)),
        None,
    )?;

    Ok(RecordBatch::try_new(
        schema(),
        vec![
            text(&concept.id),
            text(&concept.project),
            text(&concept.title),
            text(&concept.content),
            text(&tags),
            text(&sources),
            text(&concept.created_at),
            text(&concept.updated_at),
            Arc::new(vector),
        ],
    )?)
}

fn text_column<'a>(batch: &'a RecordBatch, name: &str) -> Result<&'a StringArray> {
    batch
        .column_by_name(name)
        .and_then(|column| column.as_any().downcast_ref::<StringArray>())
        .ok_or_else(|| anyhow!("concepts table has no text column '{name}'").into())
}

fn concepts_from_batch(batch: &RecordBatch) -> Result<Vec<Concept>> {
    let id = text_column(batch, "id")?;
    let project = text_column(batch, "project")?;
    let title = text_column(batch, "title")?;
    let content = text_column(batch, "content")?;
    let tags = text_column(batch, "tags")?;
    let sources = text_column(batch, "sources")?;
    let created_at = text_column(batch, "created_at")?;
    let updated_at = text_column(batch, "updated_at")?;

    (0..batch.num_rows())
        .map(|row| {
            Ok(Concept {
                id: id.value(row).to_string(),
                project: project.value(row).to_string(),
                title: title.value(row).to_string(),
                content: content.value(row).to_string(),
                tags: serde_json::from_str(tags.value(row))
                    .context("corrupt tags in concepts table")?,
                sources: serde_json::from_str(sources.value(row))
                    .context("corrupt sources in concepts table")?,
                created_at: created_at.value(row).to_string(),
                updated_at: updated_at.value(row).to_string(),
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Concept {
        Concept {
            id: Uuid::new_v4().to_string(),
            project: "demo".into(),
            title: "Prefer WAL".into(),
            content: "WAL lets readers proceed while a writer commits.".into(),
            tags: vec!["sqlite".into()],
            sources: vec![3, 7],
            created_at: now(),
            updated_at: now(),
        }
    }

    #[test]
    fn batch_round_trips_a_concept() {
        let concept = sample();
        let batch = batch_of(&concept, vec![0.0; VECTOR_DIM as usize]).unwrap();
        let restored = concepts_from_batch(&batch).unwrap().remove(0);
        assert_eq!(restored.id, concept.id);
        assert_eq!(restored.tags, concept.tags);
        assert_eq!(restored.sources, concept.sources);
    }

    #[test]
    fn batch_rejects_a_vector_of_the_wrong_size() {
        assert!(batch_of(&sample(), vec![0.0; 3]).is_err());
    }

    #[test]
    fn concept_id_must_be_a_uuid() {
        assert!(ConceptId::try_from(Uuid::new_v4().to_string()).is_ok());
        assert!(ConceptId::try_from("x' OR '1'='1".to_string()).is_err());
    }

    #[test]
    fn sql_literal_escapes_quotes() {
        assert_eq!(sql_literal("a'b"), "'a''b'");
    }

    #[tokio::test]
    async fn empty_store_reports_missing_concepts() {
        let dir = tempfile::tempdir().unwrap();
        let store = ConceptStore::new(
            dir.path().join("concepts.lancedb"),
            dir.path().join("models"),
        );
        let id = ConceptId(Uuid::new_v4());
        assert!(matches!(
            store.forget(id).await,
            Err(MemoryError::NotFound(_))
        ));
        assert!(matches!(store.get(id).await, Err(MemoryError::NotFound(_))));
        let patch =
            serde_json::from_value(serde_json::json!({"id": id.0.to_string(), "title": "x"}))
                .unwrap();
        assert!(matches!(
            store.update(patch).await,
            Err(MemoryError::NotFound(_))
        ));
    }

    #[tokio::test]
    #[ignore = "downloads the embedding model"]
    async fn concepts_are_found_by_meaning_within_scope() {
        let dir = tempfile::tempdir().unwrap();
        let models = std::env::var_os("PENTACORE_TEST_MODELS")
            .map(PathBuf::from)
            .unwrap_or_else(|| dir.path().join("models"));
        let store = ConceptStore::new(dir.path().join("concepts.lancedb"), models);
        let project = ProjectName::try_from("demo".to_string()).unwrap();
        let new = |value: serde_json::Value| serde_json::from_value::<NewConcept>(value).unwrap();

        let wal = store
            .memorize(
                new(serde_json::json!({"title": "Prefer WAL", "content": "Write-ahead logging lets database readers proceed during writes."})),
                project.clone(),
            )
            .await
            .unwrap();
        assert!(wal.similar.is_empty());
        let wal = wal.concept;
        let again = store
            .memorize(
                new(serde_json::json!({"title": "Prefer WAL", "content": "Write-ahead logging lets database readers proceed during writes."})),
                project.clone(),
            )
            .await
            .unwrap();
        assert_eq!(again.similar[0].id, wal.id);
        store
            .forget(ConceptId::try_from(again.concept.id).unwrap())
            .await
            .unwrap();
        store
            .memorize(
                new(serde_json::json!({"title": "Sourdough", "content": "Bread dough needs a long cold fermentation.", "project": "kitchen"})),
                project.clone(),
            )
            .await
            .unwrap();

        let search = |value: serde_json::Value| {
            store.search(
                serde_json::from_value(value).unwrap(),
                Scope::Project(project.clone()),
            )
        };
        let hits = search(serde_json::json!({"query": "concurrent access to sqlite"}))
            .await
            .unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].concept.id, wal.id);
        let all = search(serde_json::json!({"query": "baking bread", "project": "*"}))
            .await
            .unwrap();
        assert_eq!(all[0].concept.title, "Sourdough");

        let id = ConceptId::try_from(wal.id.clone()).unwrap();
        let patch =
            serde_json::from_value(serde_json::json!({"id": wal.id, "title": "Use WAL mode"}))
                .unwrap();
        assert_eq!(store.update(patch).await.unwrap().title, "Use WAL mode");
        assert_eq!(store.get(id).await.unwrap().title, "Use WAL mode");
        store.forget(id).await.unwrap();
        assert!(matches!(store.get(id).await, Err(MemoryError::NotFound(_))));
    }
}
