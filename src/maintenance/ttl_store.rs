//! TTL（有効期限）付きのスレッドセーフなインメモリ KV ストア。全リポジトリ/ストレージの実体。
//!
//! - 中身は `DashMap<K, Entry<Arc<V>>>`。値は `Arc` で返すので取得時に大きな画像データをコピーしない
//! - **`get` するたびに最終アクセス時刻が更新される**（sliding expiration）。`insert` でも更新
//! - `contains` は時刻を更新しない
//! - 実際の削除は `delete_expired` が呼ばれたときだけ（GC タスクが 60 秒ごとに呼ぶ）。
//!   期限切れでも GC 前なら `get` で取れるし、取れた瞬間に寿命が延びる
//! - 値の更新は「`get` で Arc を取得 → clone して変更 → `insert` で丸ごと置き換え」で行う。
//!   同じキーへの並行更新はロックされないので、後から `insert` した方が勝つ（lost update）。
//!   編集セッションは `EditingSessionLocks` でセッション単位に直列化して防いでいる
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

    /// キーがあればその値を、無ければ `make()` で作って挿入した値を返す。最終アクセス時刻も更新する。
    /// 「確認 → 挿入」を DashMap の entry API で原子的に行うので、同じキーで同時に呼ばれても
    /// 全員が同じ `Arc` を受け取る（ロック表などに使う）。
    pub fn get_or_insert_with(&self, key: K, make: impl FnOnce() -> V) -> Arc<V> {
        let now = self.clock.now();
        let mut entry = self
            .map
            .entry(key)
            .or_insert_with(|| Entry::new(Arc::new(make()), now));
        entry.touch(now);
        Arc::clone(&entry)
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

#[cfg(test)]
mod ttl_store_test {
    use std::{sync::Arc, time::Instant};

    use super::TtlStore;
    use crate::maintenance::deleting_expired_entries::clock::clock_impl::FakeClock;

    #[test]
    fn get_or_insert_with_returns_the_same_value_for_the_same_key() {
        let store: TtlStore<u32, String> = TtlStore::new(Arc::new(FakeClock::new(Instant::now())));

        let first = store.get_or_insert_with(1, || "first".to_string());
        let second = store.get_or_insert_with(1, || "second".to_string());

        assert!(Arc::ptr_eq(&first, &second));
        assert_eq!(*second, "first");
    }
}
