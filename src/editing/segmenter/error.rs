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
