use crate::{editing::segmenter::error::PrepareServiceError, parent_error};

parent_error!(
    pub enum PrepareSegmentUseCaseError {
        PrepareSegmentService(PrepareServiceError),
    }
);
