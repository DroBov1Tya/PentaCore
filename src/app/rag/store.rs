use anyhow::Result;
use arrow_array::{
    FixedSizeListArray, RecordBatch, RecordBatchIterator, StringArray, types::Float32Type,
};
use arrow_schema::{DataType, Field, Schema};
use futures::TryStreamExt;
use lancedb::{
    Table,
    connection::Connection,
    index::Index,
    query::{ExecutableQuery, QueryBase},
};
use std::sync::Arc;
use uuid::Uuid;

use super::embedder::LocalEmbedder;
use super::types::{MemoryNote, SearchResult};

pub struct MemoryStore {
    db: Connection,
    table_name: String,
    embedder: Option<LocalEmbedder>,
}

impl MemoryStore {
    pub async fn new(db_path: &str) -> Result<Self> {
        let db = lancedb::connect(db_path).execute().await?;
        let table_name = "memories".to_string();

        Ok(Self {
            db,
            table_name,
            embedder: None,
        })
    }

    fn get_embedder(&mut self) -> Result<&mut LocalEmbedder> {
        if self.embedder.is_none() {
            tracing::info!("Initializing fastembed ONNX model on first use...");
            self.embedder = Some(LocalEmbedder::new()?);
        }
        Ok(self.embedder.as_mut().unwrap())
    }

    async fn get_or_create_table(&self) -> Result<Table> {
        let table_names = self.db.table_names().execute().await?;
        if table_names.contains(&self.table_name) {
            Ok(self.db.open_table(&self.table_name).execute().await?)
        } else {
            let schema = Arc::new(Schema::new(vec![
                Field::new("id", DataType::Utf8, false),
                Field::new("domain", DataType::Utf8, false),
                Field::new("category", DataType::Utf8, false),
                Field::new("title", DataType::Utf8, false),
                Field::new("content", DataType::Utf8, false),
                Field::new("tags", DataType::Utf8, false),
                Field::new(
                    "vector",
                    DataType::FixedSizeList(
                        Arc::new(Field::new("item", DataType::Float32, true)),
                        384,
                    ),
                    false,
                ),
            ]));

            let empty_batch = RecordBatch::new_empty(schema.clone());

            Ok(self
                .db
                .create_table(&self.table_name, vec![empty_batch])
                .execute()
                .await?)
        }
    }

    // Embeds content into the vector DB so the agent can find it later via search().
    pub async fn memorize(
        &mut self,
        domain: &str,
        category: &str,
        title: &str,
        content: &str,
        tags: &[String],
    ) -> Result<String> {
        let id = Uuid::new_v4().to_string();
        self.memorize_with_id(&id, domain, category, title, content, tags)
            .await?;
        Ok(id)
    }

    // Same as memorize() but takes an explicit id. Use when you need a stable, deterministic ID
    // that survives SQLite resets (e.g. the knowledge seeder).
    pub async fn memorize_with_id(
        &mut self,
        id: &str,
        domain: &str,
        category: &str,
        title: &str,
        content: &str,
        tags: &[String],
    ) -> Result<()> {
        let table = self.get_or_create_table().await?;

        let embed_text = format!("{}: {}", title, content);
        let vector = self.get_embedder()?.embed(&embed_text)?;
        let tags_json = serde_json::to_string(tags)?;

        let schema = table.schema().await?;

        let id_arr = StringArray::from(vec![id.to_string()]);
        let domain_arr = StringArray::from(vec![domain]);
        let cat_arr = StringArray::from(vec![category]);
        let title_arr = StringArray::from(vec![title]);
        let content_arr = StringArray::from(vec![content]);
        let tags_arr = StringArray::from(vec![tags_json]);

        let vec_opt: Vec<Option<Vec<Option<f32>>>> =
            vec![Some(vector.into_iter().map(Some).collect())];
        let vector_arr = FixedSizeListArray::from_iter_primitive::<Float32Type, _, _>(vec_opt, 384);

        let batch = RecordBatch::try_new(
            schema.clone(),
            vec![
                Arc::new(id_arr),
                Arc::new(domain_arr),
                Arc::new(cat_arr),
                Arc::new(title_arr),
                Arc::new(content_arr),
                Arc::new(tags_arr),
                Arc::new(vector_arr),
            ],
        )?;

        table.add(vec![batch]).execute().await?;

        Ok(())
    }

    /// Performs a semantic search filtered by category (e.g., "lesson", "technique").
    /// Used by methodology layer to retrieve only structured episodic memories.
    pub async fn search_by_category(
        &mut self,
        query: &str,
        category: &str,
        domain_filter: Option<&str>,
        limit: usize,
    ) -> Result<SearchResult> {
        let table = self.get_or_create_table().await?;
        if table.count_rows(None).await? == 0 {
            return Ok(SearchResult {
                query: query.to_string(),
                results: vec![],
                total_memories: 0,
            });
        }

        let query_vector = self.get_embedder()?.embed(query)?;
        let total_memories = table.count_rows(None).await? as i64;

        let filter = if let Some(d) = domain_filter {
            format!("category = '{}' AND domain = '{}'", category, d)
        } else {
            format!("category = '{}'", category)
        };

        let batches: Vec<RecordBatch> = table
            .query()
            .nearest_to(query_vector.as_slice())?
            .limit(limit)
            .only_if(filter)
            .execute()
            .await?
            .try_collect()
            .await?;

        let mut results = Vec::new();
        for batch in batches {
            let ids = batch
                .column(0)
                .as_any()
                .downcast_ref::<StringArray>()
                .unwrap();
            let domains = batch
                .column(1)
                .as_any()
                .downcast_ref::<StringArray>()
                .unwrap();
            let cats = batch
                .column(2)
                .as_any()
                .downcast_ref::<StringArray>()
                .unwrap();
            let titles = batch
                .column(3)
                .as_any()
                .downcast_ref::<StringArray>()
                .unwrap();
            let contents = batch
                .column(4)
                .as_any()
                .downcast_ref::<StringArray>()
                .unwrap();
            let tags = batch
                .column(5)
                .as_any()
                .downcast_ref::<StringArray>()
                .unwrap();
            let distance_col = batch.column_by_name("_distance");

            for i in 0..batch.num_rows() {
                let parsed_tags: Vec<String> =
                    serde_json::from_str(tags.value(i)).unwrap_or_default();
                let dist = distance_col
                    .and_then(|c| c.as_any().downcast_ref::<arrow_array::Float32Array>())
                    .map(|arr| arr.value(i) as f64);

                if let Some(d) = dist {
                    // AllMiniLML6V2 without explicit normalization produces distances in ~1.4–1.7
                    // range even for semantically close matches. Threshold 1.7 passes relevant
                    // results while filtering near-orthogonal noise (d > 1.8+).
                    if d > 1.7 {
                        continue;
                    }
                }

                results.push(MemoryNote {
                    id: Some(ids.value(i).to_string()),
                    domain: domains.value(i).to_string(),
                    category: cats.value(i).to_string(),
                    title: titles.value(i).to_string(),
                    content: contents.value(i).to_string(),
                    tags: parsed_tags,
                    score: dist,
                });
            }
        }

        Ok(SearchResult {
            query: query.to_string(),
            results,
            total_memories,
        })
    }

    /// Performs a semantic search (RAG) against the vector database using a text query.
    /// Converts the query into a vector representation and finds the `limit` nearest neighbors.
    /// Can optionally filter by `domain` to restrict knowledge retrieval to a specific target.
    pub async fn search(
        &mut self,
        query: &str,
        domain_filter: Option<&str>,
        limit: usize,
    ) -> Result<SearchResult> {
        let table = self.get_or_create_table().await?;
        if table.count_rows(None).await? == 0 {
            return Ok(SearchResult {
                query: query.to_string(),
                results: vec![],
                total_memories: 0,
            });
        }

        let query_vector = self.get_embedder()?.embed(query)?;
        let total_memories = table.count_rows(None).await? as i64;

        let mut q = table.query().nearest_to(query_vector.as_slice())?;

        let batches: Vec<RecordBatch> = if let Some(d) = domain_filter {
            q.limit(limit)
                .only_if(format!("domain = '{}'", d))
                .execute()
                .await?
                .try_collect()
                .await?
        } else {
            q.limit(limit).execute().await?.try_collect().await?
        };

        let mut results = Vec::new();
        for batch in batches {
            let ids = batch
                .column(0)
                .as_any()
                .downcast_ref::<StringArray>()
                .unwrap();
            let domains = batch
                .column(1)
                .as_any()
                .downcast_ref::<StringArray>()
                .unwrap();
            let cats = batch
                .column(2)
                .as_any()
                .downcast_ref::<StringArray>()
                .unwrap();
            let titles = batch
                .column(3)
                .as_any()
                .downcast_ref::<StringArray>()
                .unwrap();
            let contents = batch
                .column(4)
                .as_any()
                .downcast_ref::<StringArray>()
                .unwrap();
            let tags = batch
                .column(5)
                .as_any()
                .downcast_ref::<StringArray>()
                .unwrap();

            let distance_col = batch.column_by_name("_distance");

            for i in 0..batch.num_rows() {
                let parsed_tags: Vec<String> =
                    serde_json::from_str(tags.value(i)).unwrap_or_default();

                let dist = distance_col
                    .and_then(|c| c.as_any().downcast_ref::<arrow_array::Float32Array>())
                    .map(|arr| arr.value(i) as f64);

                if let Some(d) = dist {
                    if d > 1.7 {
                        continue;
                    }
                }

                results.push(MemoryNote {
                    id: Some(ids.value(i).to_string()),
                    domain: domains.value(i).to_string(),
                    category: cats.value(i).to_string(),
                    title: titles.value(i).to_string(),
                    content: contents.value(i).to_string(),
                    tags: parsed_tags,
                    score: dist,
                });
            }
        }

        Ok(SearchResult {
            query: query.to_string(),
            results,
            total_memories,
        })
    }

    /// Retrieves the most recently added memories from the vector database without semantic filtering.
    /// Useful for recalling what was just added to the long-term memory.
    pub async fn list_memories(
        &self,
        domain_filter: Option<&str>,
        limit: usize,
    ) -> Result<Vec<MemoryNote>> {
        let table = self.get_or_create_table().await?;
        if table.count_rows(None).await? == 0 {
            return Ok(vec![]);
        }

        let mut q = table.query();
        let batches: Vec<RecordBatch> = if let Some(d) = domain_filter {
            q.limit(limit)
                .only_if(format!("domain = '{}'", d))
                .execute()
                .await?
                .try_collect()
                .await?
        } else {
            q.limit(limit).execute().await?.try_collect().await?
        };

        let mut results = Vec::new();
        for batch in batches {
            let ids = batch
                .column(0)
                .as_any()
                .downcast_ref::<StringArray>()
                .unwrap();
            let domains = batch
                .column(1)
                .as_any()
                .downcast_ref::<StringArray>()
                .unwrap();
            let cats = batch
                .column(2)
                .as_any()
                .downcast_ref::<StringArray>()
                .unwrap();
            let titles = batch
                .column(3)
                .as_any()
                .downcast_ref::<StringArray>()
                .unwrap();
            let contents = batch
                .column(4)
                .as_any()
                .downcast_ref::<StringArray>()
                .unwrap();
            let tags = batch
                .column(5)
                .as_any()
                .downcast_ref::<StringArray>()
                .unwrap();

            for i in 0..batch.num_rows() {
                let parsed_tags: Vec<String> =
                    serde_json::from_str(tags.value(i)).unwrap_or_default();

                results.push(MemoryNote {
                    id: Some(ids.value(i).to_string()),
                    domain: domains.value(i).to_string(),
                    category: cats.value(i).to_string(),
                    title: titles.value(i).to_string(),
                    content: contents.value(i).to_string(),
                    tags: parsed_tags,
                    score: None,
                });
            }
        }

        Ok(results)
    }

    pub async fn get_memory(&self, memory_id: &str) -> Result<Option<MemoryNote>> {
        let table = self.get_or_create_table().await?;
        let batches: Vec<RecordBatch> = table
            .query()
            .only_if(format!("id = '{}'", memory_id))
            .limit(1)
            .execute()
            .await?
            .try_collect()
            .await?;

        for batch in batches {
            if batch.num_rows() > 0 {
                let ids = batch
                    .column(0)
                    .as_any()
                    .downcast_ref::<StringArray>()
                    .unwrap();
                let domains = batch
                    .column(1)
                    .as_any()
                    .downcast_ref::<StringArray>()
                    .unwrap();
                let cats = batch
                    .column(2)
                    .as_any()
                    .downcast_ref::<StringArray>()
                    .unwrap();
                let titles = batch
                    .column(3)
                    .as_any()
                    .downcast_ref::<StringArray>()
                    .unwrap();
                let contents = batch
                    .column(4)
                    .as_any()
                    .downcast_ref::<StringArray>()
                    .unwrap();
                let tags = batch
                    .column(5)
                    .as_any()
                    .downcast_ref::<StringArray>()
                    .unwrap();

                let parsed_tags: Vec<String> =
                    serde_json::from_str(tags.value(0)).unwrap_or_default();

                return Ok(Some(MemoryNote {
                    id: Some(ids.value(0).to_string()),
                    domain: domains.value(0).to_string(),
                    category: cats.value(0).to_string(),
                    title: titles.value(0).to_string(),
                    content: contents.value(0).to_string(),
                    tags: parsed_tags,
                    score: None,
                }));
            }
        }
        Ok(None)
    }

    pub async fn update_memory(
        &mut self,
        memory_id: &str,
        category: Option<&str>,
        title: Option<&str>,
        content: Option<&str>,
        tags: Option<&[String]>,
    ) -> Result<()> {
        let existing = self
            .get_memory(memory_id)
            .await?
            .ok_or_else(|| anyhow::anyhow!("Memory with id {} not found", memory_id))?;

        let new_cat = category.unwrap_or(&existing.category);
        let new_title = title.unwrap_or(&existing.title);
        let new_content = content.unwrap_or(&existing.content);
        let new_tags = tags.unwrap_or(&existing.tags);

        self.forget(memory_id).await?;

        let table = self.get_or_create_table().await?;
        let embed_text = format!("{}: {}", new_title, new_content);
        let vector = self.get_embedder()?.embed(&embed_text)?;
        let tags_json = serde_json::to_string(new_tags)?;

        let schema = table.schema().await?;

        let id_arr = StringArray::from(vec![memory_id.to_string()]);
        let domain_arr = StringArray::from(vec![existing.domain.as_str()]);
        let cat_arr = StringArray::from(vec![new_cat]);
        let title_arr = StringArray::from(vec![new_title]);
        let content_arr = StringArray::from(vec![new_content]);
        let tags_arr = StringArray::from(vec![tags_json]);

        let vec_opt: Vec<Option<Vec<Option<f32>>>> =
            vec![Some(vector.into_iter().map(Some).collect())];
        let vector_arr =
            FixedSizeListArray::from_iter_primitive::<arrow_array::types::Float32Type, _, _>(
                vec_opt, 384,
            );

        let batch = RecordBatch::try_new(
            schema.clone(),
            vec![
                Arc::new(id_arr),
                Arc::new(domain_arr),
                Arc::new(cat_arr),
                Arc::new(title_arr),
                Arc::new(content_arr),
                Arc::new(tags_arr),
                Arc::new(vector_arr),
            ],
        )?;

        table.add(vec![batch]).execute().await?;
        Ok(())
    }

    pub async fn forget(&self, memory_id: &str) -> Result<()> {
        let table = self.get_or_create_table().await?;
        table
            .delete(format!("id = '{}'", memory_id).as_str())
            .await?;
        Ok(())
    }
}
