//! 編集セッション: SAM2 の画像特徴量（不変）+ ポイント履歴 + マスク履歴（undo/redo 対象）。

pub mod error;
pub mod inference_context_history;
pub mod repository;
pub mod session;
