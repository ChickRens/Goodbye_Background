use crate::{common::traits::ErrorType::Internal, leaf_detail_error};

leaf_detail_error!(
    pub enum SegmenterModelError {
        ModelLoadError => ("MODEL_LOAD_ERROR", Internal),
    }
);

leaf_detail_error!(
    pub enum SegmenterRuntimeError {
        InferenceError => ("INFERENCE_ERROR", Internal),
    }
);

leaf_detail_error!(
    pub enum SegmenterLoadingError {
        PreProcessError => ("PREPROCESS_ERROR", Internal),
    }
);

parent_error!(
    pub enum SegmentServiceError {
        SessionRepository(SessionRepositoryError),
        ImageRepository(OriginalImageRepositoryError),
        SegmenterInputStorage(SegmenterInputImageStorageError),
        EditingSessionRepository(EditingSessionRepositoryError),
        Segmenter(SegmenterRuntimeError),
        EditingSession(EditingSessionError),
    }
);

parent_error!(
    pub enum PrepareServiceError {
        Segmenter(SegmenterLoadingError),
        ImageRepository(OriginalImageRepositoryError),
        EditingSessionRepository(EditingSessionRepositoryError),
        SegmenterInputStorage(SegmenterInputImageStorageError),
    }
);
