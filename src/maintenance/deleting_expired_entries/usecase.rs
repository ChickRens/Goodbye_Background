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

pub struct DeleteExpiredEntriesUseCase<IR, SR, CIR, PS, SS>
where
    IR: OriginalImageRepository + DeleteExpiredRepository,
    SR: SessionRepository + DeleteExpiredRepository,
    CIR: CompletedImageRepository + DeleteExpiredRepository,
    PS: PreviewStorage + DeleteExpiredRepository,
    SS: SegmenterInputImageStorage + DeleteExpiredRepository,
{
    image_repo: IR,
    session_repo: SR,
    completed_repo: CIR,
    preview_storage: PS,
    segmenter_input_storage: SS,
    ttl: Duration,
}

impl<IR, SR, CIR, PS, SS> DeleteExpiredEntriesUseCase<IR, SR, CIR, PS, SS>
where
    IR: OriginalImageRepository + DeleteExpiredRepository,
    SR: SessionRepository + DeleteExpiredRepository,
    CIR: CompletedImageRepository + DeleteExpiredRepository,
    PS: PreviewStorage + DeleteExpiredRepository,
    SS: SegmenterInputImageStorage + DeleteExpiredRepository,
{
    pub fn new(
        image_repository: IR,
        session_repository: SR,
        completed_image_repository: CIR,
        preview_storage: PS,
        segmenter_input_storage: SS,
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
