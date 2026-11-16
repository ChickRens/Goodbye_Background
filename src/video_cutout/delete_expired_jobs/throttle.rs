//! `DeleteExpiredVideoJobs` を動画 API のリクエスト処理中に 60 秒に 1 回までにする（仕様 §4.9）。

use std::sync::atomic::{AtomicU64, Ordering};

/// 実行間隔（ミリ秒）。
pub const CLEANUP_INTERVAL_MS: u64 = 60_000;
/// まだ一度も実行していない印。
const NEVER: u64 = u64::MAX;

pub struct VideoCleanupThrottle {
    last_run_ms: AtomicU64,
}

impl VideoCleanupThrottle {
    pub fn new() -> Self {
        Self {
            last_run_ms: AtomicU64::new(NEVER),
        }
    }

    /// 最後に `true` を返してから `CLEANUP_INTERVAL_MS` 以上経っていれば `true`（初回は常に `true`）。
    /// 同時に呼ばれても `true` を返すのは 1 つだけ。
    pub fn should_run(&self, now_ms: u64) -> bool {
        let mut last = self.last_run_ms.load(Ordering::SeqCst);
        loop {
            if last != NEVER && now_ms < last.saturating_add(CLEANUP_INTERVAL_MS) {
                return false;
            }
            match self.last_run_ms.compare_exchange(
                last,
                now_ms,
                Ordering::SeqCst,
                Ordering::SeqCst,
            ) {
                Ok(_) => return true,
                Err(current) => last = current,
            }
        }
    }
}

#[cfg(test)]
mod video_cleanup_throttle_test {
    use super::*;

    #[test]
    fn throttle_runs_once_per_minute() {
        let t = VideoCleanupThrottle::new();
        assert!(t.should_run(0));
        assert!(!t.should_run(59_999));
        assert!(t.should_run(60_000));
        assert!(!t.should_run(60_001));
        assert!(t.should_run(200_000));
    }

    #[test]
    fn first_call_runs_at_any_time() {
        assert!(VideoCleanupThrottle::new().should_run(1_700_000_000_000));
    }
}
