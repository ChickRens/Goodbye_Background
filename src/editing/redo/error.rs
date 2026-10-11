use crate::{
    editing::segmenter::error::SegmentServiceError,
    image::segmenter_input::storage::error::SegmenterInputImageStorageError, parent_error,
};

parent_error!(
    pub enum RedoUseCaseError {
        SegmentService(SegmentServiceError),
        SegmenterInputStorage(SegmenterInputImageStorageError),
    }
);
