// The database file and how it is opened.

use std::time::Duration;

pub const BUSY_TIMEOUT: Duration = Duration::from_secs(5);
pub const FILE: &str = "brain.db";
pub const LEGACY_FILE: &str = "brain.sqlite";
pub const PLAIN_HEADER: &[u8] = b"SQLite format 3\0";
// Well below the depth where SQLite fails on cascading deletes.
pub const MAX_TREE_DEPTH: i64 = 64;
pub const CHECKPOINTS_KEPT: i64 = 20;
