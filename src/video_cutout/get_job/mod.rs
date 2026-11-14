//! `GET /video-jobs/{id}`: ジョブの状態を返す（期限切れなら `Failed(timeout)` にする）ユースケース。

pub mod error;
pub mod input;
pub mod output;
pub mod usecase;
