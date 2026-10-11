use std::time::{Duration, Instant};

pub trait DeleteExpiredRepository {
    fn delete_expired(&self, now: Instant, ttl: Duration);
}
