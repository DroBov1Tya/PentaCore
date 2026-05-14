use anyhow::Result;
use arrow_array::{
    types::Float32Type, FixedSizeListArray, RecordBatch, RecordBatchIterator, StringArray,
};
use arrow_schema::{DataType, Field, Schema};
use futures::TryStreamExt;
use lancedb::{query::{ExecutableQuery, QueryBase}, connection::Connection, index::Index, Table};
use std::sync::Arc;
use uuid::Uuid;

use super::embedder::LocalEmbedder;
use super::types::{MemoryNote, SearchResult};

pub struct MemoryStore {
    db: Connection,
    table_name: String,
    embedder: LocalEmbedder,
}

impl MemoryStore {
    pub async fn new(db_path: &str) -> Result<Self> {
        let embedder = LocalEmbedder::new()?;
        
        let db = lancedb::connect(db_path).execute().await?;
        let table_name = "memories".to_string();

        Ok(Self {
            db,
            table_name,
            embedder,
        })
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
                    DataType::FixedSizeList(Arc::new(Field::new("item", DataType::Float32, true)), 384),
                    false,
                ),
            ]));

            let empty_batch = RecordBatch::new_empty(schema.clone());
            
            Ok(self.db
                .create_table(&self.table_name, vec![empty_batch])
                .execute()
                .await?)
        }
    }

    pub async fn memorize(
        &mut self,
        domain: &str,
        category: &str,
        title: &str,
        content: &str,
        tags: &[String],
    ) -> Result<String> {
        let table = self.get_or_create_table().await?;
        
        let embed_text = format!("{} — {}", title, content);
        let vector = self.embedder.embed(&embed_text)?;
        let tags_json = serde_json::to_string(tags)?;
        let id = Uuid::new_v4().to_string();

        let schema = table.schema().await?;
        
        let id_arr = StringArray::from(vec![id.clone()]);
        let domain_arr = StringArray::from(vec![domain]);
        let cat_arr = StringArray::from(vec![category]);
        let title_arr = StringArray::from(vec![title]);
        let content_arr = StringArray::from(vec![content]);
        let tags_arr = StringArray::from(vec![tags_json]);
        
        let vec_opt: Vec<Option<Vec<Option<f32>>>> = vec![Some(vector.into_iter().map(Some).collect())];
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

        Ok(id)
    }

    pub async fn search(
        &mut self,
        query: &str,
        domain_filter: Option<&str>,
        limit: usize,
    ) -> Result<SearchResult> {
        let table = self.get_or_create_table().await?;
        if table.count_rows(None).await? == 0 {
            return Ok(SearchResult { query: query.to_string(), results: vec![], total_memories: 0 });
        }

        let query_vector = self.embedder.embed(query)?;
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
            q.limit(limit)
             .execute()
             .await?
             .try_collect()
             .await?
        };
        
        let mut results = Vec::new();
        for batch in batches {
            let ids = batch.column(0).as_any().downcast_ref::<StringArray>().unwrap();
            let domains = batch.column(1).as_any().downcast_ref::<StringArray>().unwrap();
            let cats = batch.column(2).as_any().downcast_ref::<StringArray>().unwrap();
            let titles = batch.column(3).as_any().downcast_ref::<StringArray>().unwrap();
            let contents = batch.column(4).as_any().downcast_ref::<StringArray>().unwrap();
            let tags = batch.column(5).as_any().downcast_ref::<StringArray>().unwrap();
            
            let distance_col = batch.column_by_name("_distance");

            for i in 0..batch.num_rows() {
                let parsed_tags: Vec<String> = serde_json::from_str(tags.value(i)).unwrap_or_default();
                
                let dist = distance_col
                    .and_then(|c| c.as_any().downcast_ref::<arrow_array::Float32Array>())
                    .map(|arr| arr.value(i) as f64);

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

    pub async fn list_memories(&self, domain_filter: Option<&str>, limit: usize) -> Result<Vec<MemoryNote>> {
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
            q.limit(limit)
             .execute()
             .await?
             .try_collect()
             .await?
        };

        let mut results = Vec::new();
        for batch in batches {
            let ids = batch.column(0).as_any().downcast_ref::<StringArray>().unwrap();
            let domains = batch.column(1).as_any().downcast_ref::<StringArray>().unwrap();
            let cats = batch.column(2).as_any().downcast_ref::<StringArray>().unwrap();
            let titles = batch.column(3).as_any().downcast_ref::<StringArray>().unwrap();
            let contents = batch.column(4).as_any().downcast_ref::<StringArray>().unwrap();
            let tags = batch.column(5).as_any().downcast_ref::<StringArray>().unwrap();

            for i in 0..batch.num_rows() {
                let parsed_tags: Vec<String> = serde_json::from_str(tags.value(i)).unwrap_or_default();

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

    pub async fn forget(&self, memory_id: &str) -> Result<()> {
        let table = self.get_or_create_table().await?;
        table.delete(format!("id = '{}'", memory_id).as_str()).await?;
        Ok(())
    }
}
