//! プレビューのマスクのエラー。

use crate::{
    parent_error,
    video_cutout::{
        job::error::VideoJobError,
        preview::error::{PreviewError, PreviewInputError},
    },
};

parent_error!(
    pub enum PreviewMaskError {
        Job(VideoJobError),
        Input(PreviewInputError),
        Preview(PreviewError),
    }
);
