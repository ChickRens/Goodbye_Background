//! 流入元（utm 付きの URL で来た訪問）の記録（visit_records テーブル）。
//!
//! 流れ: フロント（`frontend/js/visit.js`）が `POST /track-visit` に utm を送る
//! → `api::handler::analytics::track_visit` が `VisitEvent` に整える
//! → `VisitRecorder::record`（非ブロッキング）→ バックグラウンドタスクが DB に INSERT。
//! 集計は `scripts/utm_report_{postgres,sqlite}.sql`。

pub mod event;
pub mod recorder;
pub mod visit_recorder;
