//! `POST /internal/video-jobs/{id}/events`: ワーカーからの通知でジョブの状態を進めるユースケース。

pub mod error;
pub mod input;
pub mod output;
pub mod usecase;
