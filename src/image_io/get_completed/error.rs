use crate::{
    application::interface::completed_image_repository::error::CompletedImageRepositoryError,
    parent_error,
};

parent_error!(
    pub enum GetCompletedUseCaseError {
        CompletedService(CompletedImageRepositoryError),
    }
);
