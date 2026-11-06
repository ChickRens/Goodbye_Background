//! `VideoClock` の実装。本番用の `RealVideoClock` とテスト用の `FakeVideoClock`。

use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::video_cutout::clock::VideoClock;

pub struct RealVideoClock;

impl VideoClock for RealVideoClock {
    fn now_ms(&self) -> u64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0)
    }
}

/// 時刻を手動で進められるテスト用の時計。
pub struct FakeVideoClock {
    ms: AtomicU64,
}

impl FakeVideoClock {
    pub fn new(ms: u64) -> Self {
        Self {
            ms: AtomicU64::new(ms),
        }
    }

    pub fn advance(&self, ms: u64) {
        self.ms.fetch_add(ms, Ordering::SeqCst);
    }
}

impl VideoClock for FakeVideoClock {
    fn now_ms(&self) -> u64 {
        self.ms.load(Ordering::SeqCst)
    }
}

#[cfg(test)]
mod clock_impl_test {
    use super::*;

    #[test]
    fn fake_clock_advances() {
        let c = FakeVideoClock::new(5);
        c.advance(10);
        assert_eq!(c.now_ms(), 15);
    }

    #[test]
    fn real_clock_returns_recent_unix_ms() {
        assert!(RealVideoClock.now_ms() > 1_700_000_000_000);
    }
}
