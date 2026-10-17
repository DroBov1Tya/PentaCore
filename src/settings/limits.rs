// Caps on what a caller may send: lengths, counts, durations.

pub const MAX_TITLE_CHARS: usize = 200;
pub const MAX_BODY_BYTES: usize = 64 * 1024;
pub const MAX_TAG_CHARS: usize = 32;
pub const MAX_PROJECT_CHARS: usize = 64;
pub const MAX_QUERY_CHARS: usize = 512;
pub const MAX_LIMIT: u64 = 100;
pub const MAX_GRAPH_DEPTH: u8 = 32;
pub const MAX_TOKEN_CHARS: usize = 48;
pub const MAX_AUTHOR_CHARS: usize = 64;
pub const MAX_BASIS_CHARS: usize = 500;
pub const MAX_KEY_CHARS: usize = 256;
pub const MAX_ATTR_NAME_CHARS: usize = 48;
pub const MAX_ATTR_TEXT_CHARS: usize = 1024;
pub const MAX_ATTRS_PER_CALL: usize = 32;
pub const MAX_ATTRS_PER_ENTITY: usize = 64;
pub const MAX_IN_VALUES: usize = 32;
pub const DEFAULT_CLAIM_SECONDS: u32 = 15 * 60;
pub const MAX_CLAIM_SECONDS: u32 = 24 * 60 * 60;
pub const MAX_CHECKS_PER_CALL: usize = 32;
pub const MAX_ITEMS_PER_NOTE: i64 = 100;
pub const MAX_REPORT_IDS: usize = 2048;
pub const DEFAULT_STALE_DAYS: u32 = 14;
pub const MAX_STALE_DAYS: u32 = 365;
