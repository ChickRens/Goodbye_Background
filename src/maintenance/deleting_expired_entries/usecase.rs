use std::sync::Arc;
use std::time::{Duration, Instant};

use crate::{
    image::{
        completed::storage::storage::CompletedImageRepository,
        original::original_repository::repository::OriginalImageRepository,
        preview::storage::storage::PreviewStorage,
        segmenter_input::storage::storage::SegmenterInputImageStorage,
    },
    maintenance::deleting_expired_entries::repository::DeleteExpiredRepository,
    session::repository::repository::SessionRepository,
};

pub trait ExpiringOriginalImageRepository:
    OriginalImageRepository + DeleteExpiredRepository
{
}
impl<T> ExpiringOriginalImageRepository for T where
    T: OriginalImageRepository + DeleteExpiredRepository
{
}

pub trait ExpiringSessionRepository: SessionRepository + DeleteExpiredRepository {}
impl<T> ExpiringSessionRepository for T where T: SessionRepository + DeleteExpiredRepository {}

pub trait ExpiringCompletedImageRepository:
    CompletedImageRepository + DeleteExpiredRepository
{
}
impl<T> ExpiringCompletedImageRepository for T where
    T: CompletedImageRepository + DeleteExpiredRepository
{
}

pub trait ExpiringPreviewStorage: PreviewStorage + DeleteExpiredRepository {}
impl<T> ExpiringPreviewStorage for T where T: PreviewStorage + DeleteExpiredRepository {}

pub trait ExpiringSegmenterInputStorage:
    SegmenterInputImageStorage + DeleteExpiredRepository
{
}
impl<T> ExpiringSegmenterInputStorage for T where
    T: SegmenterInputImageStorage + DeleteExpiredRepository
{
}

pub struct DeleteExpiredEntriesUseCase {
    image_repo: Arc<dyn ExpiringOriginalImageRepository>,
    session_repo: Arc<dyn ExpiringSessionRepository>,
    completed_repo: Arc<dyn ExpiringCompletedImageRepository>,
    preview_storage: Arc<dyn ExpiringPreviewStorage>,
    segmenter_input_storage: Arc<dyn ExpiringSegmenterInputStorage>,
    ttl: Duration,
}

impl DeleteExpiredEntriesUseCase {
    pub fn new(
        image_repository: Arc<dyn ExpiringOriginalImageRepository>,
        session_repository: Arc<dyn ExpiringSessionRepository>,
        completed_image_repository: Arc<dyn ExpiringCompletedImageRepository>,
        preview_storage: Arc<dyn ExpiringPreviewStorage>,
        segmenter_input_storage: Arc<dyn ExpiringSegmenterInputStorage>,
        ttl: Duration,
    ) -> Self {
        Self {
            image_repo: image_repository,
            session_repo: session_repository,
            completed_repo: completed_image_repository,
            preview_storage,
            segmenter_input_storage,
            ttl,
        }
    }

    pub fn execute(&self, now: Instant) {
        self.image_repo.delete_expired(now, self.ttl);
        self.session_repo.delete_expired(now, self.ttl);
        self.completed_repo.delete_expired(now, self.ttl);
        self.preview_storage.delete_expired(now, self.ttl);
        self.segmenter_input_storage.delete_expired(now, self.ttl);
    }
}
