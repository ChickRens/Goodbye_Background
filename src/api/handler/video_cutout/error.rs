//! 動画のハンドラ（HTTP 層）で起きるエラー。

use crate::{
    common::traits::ErrorType,
    leaf_error, parent_error,
    video_cutout::{
        job::error::VideoJobError, preview::error::PreviewInputError, signature::SignatureError,
    },
};

leaf_error!(
    pub enum VideoRequestError {
        MissingClientId => ("MISSING_CLIENT_ID", ErrorType::InvalidInput),
        JobIdMismatch => ("JOB_ID_MISMATCH", ErrorType::InvalidInput),
        InvalidBody => ("INVALID_REQUEST_BODY", ErrorType::InvalidInput),
    }
);

parent_error!(
    pub enum VideoHandlerError {
        Request(VideoRequestError),
        Signature(SignatureError),
        Job(VideoJobError),
        Preview(PreviewInputError),
    }
);
