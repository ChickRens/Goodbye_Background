use crate::{common::traits::ErrorType::NotFound, leaf_error};

leaf_error!(
    pub enum CompletedImageStorageError {
        ImageNotFound => ("IMAGE_NOT_FOUND", NotFound),
    }
);
