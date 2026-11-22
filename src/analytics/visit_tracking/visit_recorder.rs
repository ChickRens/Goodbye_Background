//! 訪問の記録先を抽象化するトレイト。
//!
//! `record` は同期関数で、呼び出し側（ハンドラ）をブロックしてはいけない。
//! 実装（`recorder.rs`）はチャネルに投げるだけで、実際の INSERT は別タスクで行う。

use crate::analytics::visit_tracking::event::VisitEvent;

pub trait VisitRecorder: Send + Sync {
    fn record(&self, event: VisitEvent);
}
