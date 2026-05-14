use anyhow::Result;
use fastembed::{EmbeddingModel, InitOptions, TextEmbedding};

pub struct LocalEmbedder {
    model: TextEmbedding,
}

impl LocalEmbedder {
    pub fn new() -> Result<Self> {
        let model = TextEmbedding::try_new(
            InitOptions::new(EmbeddingModel::AllMiniLML6V2).with_show_download_progress(true),
        )?;

        Ok(Self { model })
    }

    pub fn embed(&mut self, text: &str) -> Result<Vec<f32>> {
        let embeddings = self.model.embed(vec![text], None)?;

        if let Some(first) = embeddings.into_iter().next() {
            Ok(first)
        } else {
            anyhow::bail!("Failed to generate embedding")
        }
    }
}
