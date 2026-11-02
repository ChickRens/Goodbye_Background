//! `AppClock` の実装。
//!
//! - `RealClock`: `Instant::now()` を返す本番用
//! - `FakeClock`: 生成時に渡した時刻を常に返すテスト用（`#[cfg(test)]` ではないので本番バイナリにも含まれる）

use std::time::Instant;

use crate::maintenance::deleting_expired_entries::clock::clock::AppClock;

#[derive(Debug, Clone)]
pub struct RealClock;

impl AppClock for RealClock {
    fn now(&self) -> Instant {
        Instant::now()
    }
}

impl RealClock {
    pub fn new() -> Self {
        Self
    }
}

#[derive(Debug, Clone)]
pub struct FakeClock {
    time: Instant,
}

impl AppClock for FakeClock {
    fn now(&self) -> Instant {
        self.time
    }
}

impl FakeClock {
    pub fn new(time: Instant) -> Self {
        Self { time }
    }
}
