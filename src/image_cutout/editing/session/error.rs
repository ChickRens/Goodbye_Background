//! 編集セッションの undo/redo エラー。
//!
//! `HistoryCorrupted` は「ポイント履歴」と「推論コンテキスト履歴」の undo/redo 可否が食い違った場合。
//! 2つの履歴は常に同じ回数だけ add/undo/redo される前提なので、起きたらバグ。

use crate::{
    common::traits::ErrorType::{Internal, InvalidInput},
    leaf_error,
};

leaf_error!(
    pub enum EditingSessionError {
        UndoFailed => ("UNDO_FAILED", InvalidInput),
        RedoFailed => ("REDO_FAILED", InvalidInput),
        HistoryCorrupted => ("HISTORY_CORRUPTED", Internal),
    }
);
