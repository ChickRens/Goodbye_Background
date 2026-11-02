//! API 利用ログ（usage_records テーブル）の記録。
//!
//! 流れ: `presentation::middleware::usage_tracking::track_usage` が `UsageEvent` を作る
//! → `UsageRecorder::record`（非ブロッキング）→ バックグラウンドタスクが DB に INSERT。

pub mod event;
pub mod id;
pub mod operation;
pub mod recorder;
pub mod status;
pub mod usage_recorder;
