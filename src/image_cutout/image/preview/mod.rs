//! 画面表示用プレビュー（長辺 600px の WebP）。
//!
//! 「元画像のプレビュー」と「切り抜き結果のプレビュー」の2種類があり、別々のストアに入る。
//! フロントは `/get-preview` に image_id を渡して取得する（どちらの種類かは気にしない）。

pub mod generator;
pub mod preview;
pub mod service;
pub mod storage;
