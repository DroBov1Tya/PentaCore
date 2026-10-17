// Limits of the HTTP and stdio transports.

use std::time::Duration;

pub const HTTP_SHUTDOWN_GRACE: Duration = Duration::from_secs(5);
pub const MAX_REQUEST_BYTES: usize = 256 * 1024;
pub const REQUEST_TIMEOUT: Duration = Duration::from_secs(60);
pub const MAX_LINE_BYTES: usize = 1024 * 1024;
pub const MIN_TOKEN_BYTES: usize = 32;
