//! `POST /video-jobs/{id}/preview-frame`: プレビュー用のフレーム（1 枚目の静止画）を登録し、特徴量を作って置いておくユースケース。

pub mod error;
pub mod input;
pub mod output;
pub mod usecase;
