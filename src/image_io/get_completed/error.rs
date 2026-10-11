use crate::{image::completed::storage::error::CompletedImageStorageError, parent_error};

parent_error!(
    pub enum GetCompletedUseCaseError {
        CompletedService(CompletedImageStorageError),
    }
);
