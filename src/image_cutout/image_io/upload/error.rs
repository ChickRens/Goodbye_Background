//! アップロードユースケースのエラー。

use crate::{
    image_cutout::editing::{
        segmenter::error::SegmenterLoadingError,
        session::repository::error::EditingSessionRepositoryError,
    },
    image_cutout::image::original::original_repository::error::OriginalImageRepositoryError,
    image_cutout::image_io::upload::image_loader::error::LoadingError,
    parent_error,
    image_cutout::session::repository::error::SessionRepositoryError,
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
