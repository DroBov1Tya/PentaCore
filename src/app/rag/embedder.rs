use anyhow::Result;
use fastembed::{EmbeddingModel, InitOptions, TextEmbedding};

// AllMiniLML6V2, 384-dim. Vectors are not unit-normalized by default —
// actual L2 distances sit around 1.4–1.7 even for semantically close matches.
pub struct LocalEmbedder {
    model: TextEmbedding,
}

impl LocalEmbedder {
    pub fn new() -> Result<Self> {
        let model = TextEmbedding::try_new(
            InitOptions::new(EmbeddingModel::AllMiniLML6V2).with_show_download_progress(false),
        )?;
        Ok(Self { model })
    }

    pub fn embed(&mut self, text: &str) -> Result<Vec<f32>> {
        let embeddings = self.model.embed(vec![text], None)?;
        embeddings
            .into_iter()
            .next()
            .ok_or_else(|| anyhow::anyhow!("empty embedding result"))
    }
}
