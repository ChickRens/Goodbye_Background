use std::{
    marker::PhantomData,
    ops::{Deref, DerefMut},
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

impl<T> DerefMut for Entry<T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.value
    }
}

pub struct EntryGuard<G, T> {
    inner: G,
    phantom: PhantomData<T>,
}

impl<G, T> Deref for EntryGuard<G, T>
where
    G: Deref<Target = Entry<T>>,
{
    type Target = T;

    fn deref(&self) -> &Self::Target {
        self.inner.deref()
    }
}

impl<G, T> DerefMut for EntryGuard<G, T>
where
    G: DerefMut<Target = Entry<T>>,
{
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.inner.deref_mut()
    }
}

impl<G, T> EntryGuard<G, T>
where
    G: DerefMut<Target = Entry<T>>,
{
    pub fn new(mut entry: G, now: Instant) -> Self {
        entry.touch(now);
        Self {
            inner: entry,
            phantom: PhantomData,
        }
    }
}
