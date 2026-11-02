//! `/upload`: 画像を受け取り、セッションを作るユースケース。
//!
//! 注意: SAM2 の画像エンコード（重い処理）はこのユースケースではなく、
//! `App::upload` が続けて呼ぶ `PrepareSegmentUseCase` で行われる。

pub mod error;
pub mod image_loader;
pub mod input;
pub mod output;
pub mod usecase;
