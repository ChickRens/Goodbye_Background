//! 全インメモリストアから期限切れエントリを削除するユースケース。
//!
//! `main.rs` の GC タスクが `CLEANUP_INTERVAL`（60秒）ごとに `App::cleanup` 経由で `execute` を呼ぶ。
//!
//! `Expiring*` トレイトは「元のリポジトリトレイト + DeleteExpiredRepository」を1つの
//! trait object（`Arc<dyn ...>`）として扱うための合成トレイト（Rust は `dyn A + B` を書けないため）。
//! blanket impl があるので、両方を実装した型は自動的に該当する。
//!
//! 編集セッション（`SAM2EditingSessionRepository`）は関連型付きトレイトで `Expiring*` 合成トレイトを
//! 作りにくいので、GC に必要な `DeleteExpiredRepository` としてだけ受け取る。
//! 1セッション約16MBの SAM2 特徴量を持つ一番重いストアなので、ここから外すとメモリリークになる。

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
    editing_session_repo: Arc<dyn DeleteExpiredRepository>,
    editing_session_locks: Arc<dyn DeleteExpiredRepository>,
    ttl: Duration,
}

impl DeleteExpiredEntriesUseCase {
    pub fn new(
        image_repository: Arc<dyn ExpiringOriginalImageRepository>,
        session_repository: Arc<dyn ExpiringSessionRepository>,
        completed_image_repository: Arc<dyn ExpiringCompletedImageRepository>,
        preview_storage: Arc<dyn ExpiringPreviewStorage>,
        segmenter_input_storage: Arc<dyn ExpiringSegmenterInputStorage>,
        editing_session_repository: Arc<dyn DeleteExpiredRepository>,
        editing_session_locks: Arc<dyn DeleteExpiredRepository>,
        ttl: Duration,
    ) -> Self {
        Self {
            image_repo: image_repository,
            session_repo: session_repository,
            completed_repo: completed_image_repository,
            preview_storage,
            segmenter_input_storage,
            editing_session_repo: editing_session_repository,
            editing_session_locks,
            ttl,
        }
    }

    pub fn execute(&self, now: Instant) {
        self.image_repo.delete_expired(now, self.ttl);
        self.session_repo.delete_expired(now, self.ttl);
        self.completed_repo.delete_expired(now, self.ttl);
        self.preview_storage.delete_expired(now, self.ttl);
        self.segmenter_input_storage.delete_expired(now, self.ttl);
        self.editing_session_repo.delete_expired(now, self.ttl);
        self.editing_session_locks.delete_expired(now, self.ttl);
    }
}

#[cfg(test)]
mod delete_expired_entries_usecase_test {
    use std::{
        sync::Arc,
        time::{Duration, Instant},
    };

    use super::DeleteExpiredEntriesUseCase;
    use crate::{
        editing::{
            segmenter::sam2_data::{SAM2InferenceContext, SAM2StaticContext},
            session::{
                inference_context_history::InferenceContextHistory,
                lock::EditingSessionLocks,
                repository::{
                    repository::EditingSessionRepository,
                    repository_impl::SAM2EditingSessionRepository,
                },
                session::CommonEditingSession,
            },
        },
        image::{
            completed::storage::storage_impl::CompletedRepositoryInMemory,
            original::original_repository::repository_impl::OriginalImageRepositoryInMemory,
            preview::storage::storage_impl::PreviewStorageInMemory,
            segmenter_input::storage::storage_impl::SegmenterInputStorageInMemory,
        },
        maintenance::deleting_expired_entries::clock::clock_impl::FakeClock,
        point::point_history::PointHistory,
        session::{id::SessionId, repository::repository_impl::SessionRepositoryInMemory},
    };

    #[test]
    fn execute_removes_expired_editing_sessions() {
        let base = Instant::now();
        let clock = FakeClock::new(base);
        let editing_session_repo = Arc::new(SAM2EditingSessionRepository::new(clock.clone()));
        let usecase = DeleteExpiredEntriesUseCase::new(
            Arc::new(OriginalImageRepositoryInMemory::new(Arc::new(
                clock.clone(),
            ))),
            Arc::new(SessionRepositoryInMemory::new(clock.clone())),
            Arc::new(CompletedRepositoryInMemory::new(clock.clone())),
            Arc::new(PreviewStorageInMemory::new(clock.clone())),
            Arc::new(SegmenterInputStorageInMemory::new(clock.clone())),
            editing_session_repo.clone(),
            Arc::new(EditingSessionLocks::new(clock.clone())),
            Duration::from_secs(10),
        );

        let session_id = SessionId::new();
        editing_session_repo.save(
            &session_id,
            CommonEditingSession::new(
                PointHistory::new(10),
                SAM2StaticContext::default(),
                InferenceContextHistory::new(10, SAM2InferenceContext::default()),
                1.0,
            ),
        );

        usecase.execute(base + Duration::from_secs(30));

        assert!(editing_session_repo.get(&session_id).is_err());
    }
}
