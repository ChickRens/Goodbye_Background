//! `POST /video-jobs/{id}/preview`: 登録したフレームの特徴量と点から、1 フレーム目のマスク（PNG）を返すユースケース。

pub mod error;
pub mod input;
pub mod output;
pub mod usecase;
