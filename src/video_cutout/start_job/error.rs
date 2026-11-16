//! ジョブ開始のエラー。ワーカー投入の失敗はジョブの `Failed(DispatchFailed)` にするので、ここには出ない。

use crate::{
    common::traits::ErrorType, leaf_error, parent_error,
    video_cutout::{
        job::error::VideoJobError, quota::error::QuotaError, storage::error::StorageError,
    },
};

leaf_error!(
    pub enum StartError {
        UploadNotFound => ("UPLOAD_NOT_FOUND", ErrorType::Conflict),
    }
);

parent_error!(
    pub enum StartVideoJobError {
        Start(StartError),
        Job(VideoJobError),
        Quota(QuotaError),
        Storage(StorageError),
    }
);
