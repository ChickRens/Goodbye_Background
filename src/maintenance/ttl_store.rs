//! TTL（有効期限）付きのスレッドセーフなインメモリ KV ストア。全リポジトリ/ストレージの実体。
//!
//! - 中身は `DashMap<K, Entry<Arc<V>>>`。値は `Arc` で返すので取得時に大きな画像データをコピーしない
//! - **`get` するたびに最終アクセス時刻が更新される**（sliding expiration）。`insert` でも更新
//! - `contains` は時刻を更新しない
//! - 実際の削除は `delete_expired` が呼ばれたときだけ（GC タスクが 60 秒ごとに呼ぶ）。
//!   期限切れでも GC 前なら `get` で取れるし、取れた瞬間に寿命が延びる
//! - 値の更新は「`get` で Arc を取得 → clone して変更 → `insert` で丸ごと置き換え」で行う。
//!   同じキーへの並行更新はロックされないので、後から `insert` した方が勝つ（lost update）
//! - 時刻は注入された `AppClock` から取る（テストで `FakeClock` を使うため）

use std::{
    fmt,
    hash::Hash,
    sync::Arc,
    time::{Duration, Instant},
};

use dashmap::DashMap;

use crate::maintenance::deleting_expired_entries::{clock::clock::AppClock, entry::Entry};

pub struct TtlStore<K, V> {
    map: DashMap<K, Entry<Arc<V>>>,
    clock: Arc<dyn AppClock>,
}

impl<K: Eq + Hash, V> TtlStore<K, V> {
    pub fn new(clock: Arc<dyn AppClock>) -> Self {
        Self {
            map: DashMap::new(),
            clock,
        }
    }

    pub fn insert(&self, key: K, value: V) {
        self.map
            .insert(key, Entry::new(Arc::new(value), self.clock.now()));
    }

    /// 値を取得し、同時に最終アクセス時刻を更新する（`get_mut` なので書き込みロックを取る）。
    pub fn get(&self, key: K) -> Option<Arc<V>> {
        let mut entry = self.map.get_mut(&key)?;
        entry.touch(self.clock.now());
        Some(Arc::clone(&entry))
    }

    /// 存在確認のみ。最終アクセス時刻は更新しない。
    pub fn contains(&self, key: &K) -> bool {
        self.map.contains_key(key)
    }

    /// 最終アクセスから `ttl` より長く経ったエントリを全て削除する。
    pub fn delete_expired(&self, now: Instant, ttl: Duration) {
        self.map.retain(|_, entry| !entry.is_expired(now, ttl));
    }
}

impl<K, V> fmt::Debug for TtlStore<K, V>
where
    K: Eq + Hash + fmt::Debug,
    V: fmt::Debug,
{
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("TtlStore")
            .field("map", &self.map)
            .finish_non_exhaustive()
    }
}
