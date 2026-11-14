//! プレビュー用フレームの登録のエラー。

use crate::{
    parent_error,
    video_cutout::{
        job::error::VideoJobError,
        preview::error::{PreviewError, PreviewInputError},
    },
};

parent_error!(
    pub enum SetPreviewFrameError {
        Job(VideoJobError),
        Input(PreviewInputError),
        Preview(PreviewError),
    }
);
