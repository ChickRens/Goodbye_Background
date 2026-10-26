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

    pub fn get(&self, key: K) -> Option<Arc<V>> {
        let mut entry = self.map.get_mut(&key)?;
        entry.touch(self.clock.now());
        Some(Arc::clone(&entry))
    }

    pub fn contains(&self, key: &K) -> bool {
        self.map.contains_key(key)
    }

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
