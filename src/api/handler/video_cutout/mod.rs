//! 動画切り抜きのエンドポイントごとのハンドラ。1エンドポイント = 1ディレクトリ（handler.rs / request.rs / response.rs）。
//!
//! 全ハンドラは `State<Arc<VideoCutoutApp>>` のメソッドだけを呼ぶ。動画のポートは非同期なので `spawn_blocking` は使わない
//! （プレビューの推論だけはユースケースの中で `spawn_blocking` する）。
//! 動画が無効（環境変数が足りない）なら、本文やヘッダーを見る前に 503 `VIDEO_CUTOUT_DISABLED` を返す。

pub mod create_job;
pub mod error;
pub mod get_job;
pub mod phase;
pub mod preview_mask;
pub mod report_event;
pub mod requester;
pub mod set_preview_frame;
pub mod start_job;
