use crate::{
    application::{
        interface::segmenter_input_image_storage::error::SegmenterInputImageStorageError,
        service::error::SegmentServiceError,
    },
    parent_error,
};

parent_error!(
    pub enum RedoUseCaseError {
        SegmentService(SegmentServiceError),
        SegmenterInputStorage(SegmenterInputImageStorageError),
    }
);
