//! 現在時刻を返すトレイト。本番は `RealClock`、テストは `FakeClock` を使う。

use std::time::Instant;

pub trait AppClock: Send + Sync {
    fn now(&self) -> Instant;
}
