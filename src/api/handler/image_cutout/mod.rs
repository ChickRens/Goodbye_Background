//! 画像切り抜きのエンドポイントごとのハンドラ。1エンドポイント = 1ディレクトリ（handler.rs / request.rs / response.rs）。

pub mod get_completed;
pub mod get_preview;
pub mod redo;
pub mod save;
pub mod segment;
pub mod undo;
pub mod upload;
