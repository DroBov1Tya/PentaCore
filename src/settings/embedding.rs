// The embedding model's shape and how notes are fed to it.

pub const VECTOR_DIM: usize = 384;
pub const SYNC_BATCH: usize = 16;
// The model reads about 256 tokens; more text only costs time.
pub const MAX_EMBEDDED_CHARS: usize = 2000;
