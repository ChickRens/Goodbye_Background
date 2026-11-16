//! `POST /video-jobs/{id}/start`: アップロード済みの動画で切り抜きを開始する（回数を消費してワーカーに投入する）ユースケース。

pub mod error;
pub mod input;
pub mod output;
pub mod usecase;
