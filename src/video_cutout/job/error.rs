//! 動画ジョブのドメイン層のエラー。

use crate::{common::traits::ErrorType, leaf_error};

leaf_error!(
    pub enum VideoJobError {
        InvalidId => ("VIDEO_JOB_INVALID_ID", ErrorType::InvalidInput),
        InvalidRange => ("INVALID_RANGE", ErrorType::InvalidInput),
        InvalidPoints => ("INVALID_POINTS", ErrorType::InvalidInput),
        InvalidPrompts => ("INVALID_PROMPTS", ErrorType::InvalidInput),
        InvalidBackground => ("INVALID_BACKGROUND", ErrorType::InvalidInput),
        NotFound => ("VIDEO_JOB_NOT_FOUND", ErrorType::NotFound),
        InvalidState => ("VIDEO_JOB_INVALID_STATE", ErrorType::Conflict),
    }
);
