use std::time::Duration;

pub const MAX_HISTORY: usize = 40;
pub const CLEANUP_INTERVAL: Duration = Duration::new(60, 0);
pub const TTL_SEC: Duration = Duration::new(10 * 60, 0);
