use crate::{editing::segmenter::error::SegmentServiceError, parent_error};

parent_error!(
    pub enum SaveUseCaseError {
        SegmentService(SegmentServiceError),
    }
);
