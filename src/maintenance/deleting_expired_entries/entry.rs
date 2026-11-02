//! `TtlStore` に格納される値のラッパー。最終アクセス時刻を一緒に持つ。
//!
//! `Deref` を実装しているので `Entry<T>` は `&T` として扱える。

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

    /// 最終アクセスから `ttl` より長く経っていれば true。
    /// `now < last_accessed`（GC 中に別スレッドが touch した等）の場合、`Instant` の引き算は
    /// 0 に飽和する（Rust 1.60 以降）ので「期限切れでない」扱いになる。
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
