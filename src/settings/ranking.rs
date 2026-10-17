// Weights and thresholds of `recall`: how matches are scored, merged and cut.

pub const DEFAULT_LIMIT: usize = 8;
pub const CANDIDATES_PER_STORE: usize = 40;
pub const SEMANTIC_CANDIDATES: usize = 40;
pub const PREVIEW_CHARS: usize = 300;
pub const SNIPPET_CHARS: usize = 200;
pub const SNIPPET_LEAD_CHARS: usize = 60;
pub const COMPARED_CHARS: usize = 600;
// Similarity of unrelated texts sits near the floor; of a paraphrase, near the ceiling.
pub const SIMILARITY_FLOOR: f32 = 0.25;
pub const SIMILARITY_CEILING: f32 = 0.75;
pub const AGREEMENT_BONUS: f32 = 0.25;
pub const MIN_RELEVANCE: f32 = 0.08;
// Share of the best score below which a record is not shown.
pub const WEAKEST_SHOWN: f32 = 0.3;
pub const NO_LONGER_VALID: f32 = 0.5;
pub const DISPUTED: f32 = 0.8;
pub const FRESHNESS_BONUS: f32 = 0.06;
pub const FRESHNESS_HALF_LIFE_DAYS: f32 = 45.0;
pub const TASK_BONUS: f32 = 0.2;
pub const SAME_TEXT: f32 = 0.8;
pub const SAME_TITLE_SIMILAR_TEXT: f32 = 0.5;
// A rewording is close in meaning and shares words; meaning alone would also
// merge different facts about one subject.
pub const SAME_MEANING: f32 = 0.75;
pub const SAME_MEANING_SHARED_WORDS: f32 = 0.35;
pub const MAX_CHUNKS: usize = 24;
pub const EXACT: f32 = 1.0;
pub const PREFIX: f32 = 0.8;
pub const STEM: f32 = 0.6;
pub const PARTIAL: f32 = 0.5;
pub const TITLE_BONUS: f32 = 0.1;
// Below this a word says nothing by itself ("of", "in").
pub const MIN_PREFIX_CHARS: usize = 3;
