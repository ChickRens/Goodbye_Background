//! SAM2 前処理ユースケースのエラー。

use crate::{image_cutout::editing::segmenter::error::PrepareServiceError, parent_error};

parent_error!(
    pub enum PrepareSegmentUseCaseError {
        PrepareSegmentService(PrepareServiceError),
    }
);
