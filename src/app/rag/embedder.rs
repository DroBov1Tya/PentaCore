use anyhow::Result;
use fastembed::{EmbeddingModel, InitOptions, TextEmbedding};

/// A local text embedding wrapper utilizing `fastembed`.
/// Responsible for converting text strings into 384-dimensional dense vectors
/// using the `AllMiniLML6V2` ONNX model for semantic RAG search.
pub struct LocalEmbedder {
    model: TextEmbedding,
}

impl LocalEmbedder {
    /// Initializes the embedding model. Downloads the ONNX weights if not cached locally.
    pub fn new() -> Result<Self> {
        let model = TextEmbedding::try_new(
            InitOptions::new(EmbeddingModel::AllMiniLML6V2).with_show_download_progress(false),
        )?;

        Ok(Self { model })
    }

    /// Generates a single dense vector embedding for the provided text input.
    pub fn embed(&mut self, text: &str) -> Result<Vec<f32>> {
        let embeddings = self.model.embed(vec![text], None)?;

        if let Some(first) = embeddings.into_iter().next() {
            Ok(first)
        } else {
            anyhow::bail!("Failed to generate embedding")
        }
    }
}
