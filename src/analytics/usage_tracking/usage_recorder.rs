//! 利用ログの記録先を抽象化するトレイト。
//!
//! `record` は同期関数で、呼び出し側（ミドルウェア）をブロックしてはいけない。
//! 実装（`recorder.rs`）はチャネルに投げるだけで、実際の INSERT は別タスクで行う。

use crate::analytics::usage_tracking::event::UsageEvent;

pub trait UsageRecorder: Send + Sync {
    fn record(&self, event: UsageEvent);
}
