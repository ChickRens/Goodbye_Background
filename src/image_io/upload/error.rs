use crate::{
    editing::{
        segmenter::error::SegmenterLoadingError,
        session::repository::error::EditingSessionRepositoryError,
    },
    image::original::original_repository::error::OriginalImageRepositoryError,
    image_io::upload::image_loader::error::LoadingError,
    parent_error,
    session::repository::error::SessionRepositoryError,
};

parent_error!(
    pub enum UploadUseCaseError {
        ImageRepository(OriginalImageRepositoryError),
        SessionRepository(SessionRepositoryError),
        Segmenter(SegmenterLoadingError),
        EditingSessionRepository(EditingSessionRepositoryError),
        Loader(LoadingError),
    }
);
