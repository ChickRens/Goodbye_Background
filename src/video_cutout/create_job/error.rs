//! ジョブ作成のエラー。

use crate::{
    common::traits::ErrorType, leaf_error, parent_error,
    video_cutout::{job::error::VideoJobError, quota::error::QuotaError},
};

leaf_error!(
    pub enum CreateError {
        TooLarge => ("TOO_LARGE", ErrorType::InvalidInput),
        UnsupportedContentType => ("UNSUPPORTED_CONTENT_TYPE", ErrorType::InvalidInput),
    }
);

parent_error!(
    pub enum CreateVideoJobError {
        Create(CreateError),
        Job(VideoJobError),
        Quota(QuotaError),
    }
);
