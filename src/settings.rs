// Tunable numbers, grouped by the area they govern. Changing one is a code
// change on purpose: limits on untrusted input must not depend on the environment.

pub mod embedding;
pub mod limits;
pub mod listing;
pub mod ranking;
pub mod resume;
pub mod server;
pub mod storage;
