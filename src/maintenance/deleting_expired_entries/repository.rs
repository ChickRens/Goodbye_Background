//! 期限切れエントリを削除できるストアが実装するトレイト。

use std::time::{Duration, Instant};

pub trait DeleteExpiredRepository: Send + Sync {
    fn delete_expired(&self, now: Instant, ttl: Duration);
}