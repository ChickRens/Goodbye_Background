//! 計測用のエンドポイントのハンドラ。1エンドポイント = 1ディレクトリ。
//!
//! 画像の `App` は SAM2 のモデルを読むので、モデル無しでテストできるよう `State<Arc<dyn VisitRecorder>>` を直接使う
//! （動画の `VideoCutoutApp` を分けたのと同じ理由。`presentation::router::visit_routes`）。

pub mod track_visit;
