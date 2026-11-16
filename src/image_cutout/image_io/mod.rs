//! 画像の入出力ユースケース（アップロード / プレビュー取得 / 完成画像取得）。
//! 編集操作（segment/undo/redo/save）は `crate::editing` 側。

pub mod get_completed;
pub mod get_preview;
pub mod upload;
