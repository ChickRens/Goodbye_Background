//! ジョブ状態の取得のエラー。

use crate::{
    parent_error,
    video_cutout::{job::error::VideoJobError, quota::error::QuotaError},
};

parent_error!(
    pub enum GetVideoJobError {
        Job(VideoJobError),
        Quota(QuotaError),
    }
);
