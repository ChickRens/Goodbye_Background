use crate::{editing::segmenter::error::SegmentServiceError, parent_error};

parent_error!(
    pub enum UndoUseCaseError {
        SegmentService(SegmentServiceError),
    }
);
