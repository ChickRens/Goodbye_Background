use std::{
    ops::Deref,
    time::{Duration, Instant},
};

#[derive(Debug)]
pub struct Entry<T> {
    value: T,
    last_accessed: Instant,
}

impl<T> Entry<T> {
    pub fn touch(&mut self, now: Instant) {
        self.last_accessed = now
    }

    pub fn is_expired(&self, now: Instant, ttl: Duration) -> bool {
        (now - self.last_accessed) > ttl
    }

    pub fn new(value: T, now: Instant) -> Self {
        Self {
            value,
            last_accessed: now,
        }
    }
}

impl<T> Deref for Entry<T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        &self.value
    }
}
