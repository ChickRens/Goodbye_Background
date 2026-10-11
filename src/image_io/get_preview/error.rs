use crate::{image::preview::storage::error::PreviewStorageError, parent_error};

parent_error!(
    pub enum GetPreviewUseCaseError {
        PreviewStorage(PreviewStorageError),
    }
);
