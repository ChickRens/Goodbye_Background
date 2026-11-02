//! アップロード単位のセッション（session_id → 元画像の image_id の対応表）。
//!
//! SAM2 の特徴量やポイント履歴など「編集状態」は別物で、`crate::editing::session` にある。
//! 両者は同じ `SessionId` をキーにしている。

pub mod error;
pub mod id;
pub mod repository;
pub mod session;
