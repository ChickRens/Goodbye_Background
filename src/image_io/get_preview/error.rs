use crate::{application::interface::preview_storage::error::PreviewStorageError, parent_error};

parent_error!(
    pub enum GetPreviewUseCaseError {
        PreviewStorage(PreviewStorageError),
    }
);
