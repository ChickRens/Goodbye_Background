//! エンドポイントごとのハンドラ。1エンドポイント = 1ディレクトリ（handler.rs / request.rs / response.rs）。
//!
//! ルーティングの定義は `presentation::router`。

pub mod get_completed;
pub mod get_preview;
pub mod redo;
pub mod save;
pub mod segment;
pub mod undo;
pub mod upload;
