use crate::{application::service::error::SegmentServiceError, parent_error};

parent_error!(
    pub enum SegmentUseCaseError {
        SegmentService(SegmentServiceError),
    }
);
