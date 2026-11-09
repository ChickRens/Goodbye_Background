//! 期限切れのジョブの後始末（仕様 §4.5 `DeleteExpiredVideoJobs`、§5.2）。
//!
//! ジョブごとにロックを持ったまま期限を確かめ直してから処理する。
//! - 進行中（`Queued`〜`Finishing`）: `Failed(timeout)` にして回数を返し、保存し直す（30 分後にもう一度期限が来る）。
//! - `Created` / `Done` / `Failed`: 入出力ファイルを削除し（失敗はログだけ）、プレビューの特徴量 → ストア → ロックの順に消す。

use std::sync::Arc;

use crate::video_cutout::{
    finish::finish_job,
    job::{
        locks::VideoJobLocks,
        phase::{FailureCode, JobPhase},
        repository::VideoJobRepository,
        video_job::VideoJob,
    },
    preview::feature_store::PreviewFeatureStore,
    quota::daily_quota::DailyQuota,
    storage::video_object_storage::VideoObjectStorage,
};

pub struct DeleteExpiredVideoJobsUseCase {
    storage: Arc<dyn VideoObjectStorage>,
    repository: Arc<dyn VideoJobRepository>,
    locks: Arc<VideoJobLocks>,
    quota: Arc<dyn DailyQuota>,
    features: Arc<PreviewFeatureStore>,
}

impl DeleteExpiredVideoJobsUseCase {
    pub fn new(
        storage: Arc<dyn VideoObjectStorage>,
        repository: Arc<dyn VideoJobRepository>,
        locks: Arc<VideoJobLocks>,
        quota: Arc<dyn DailyQuota>,
        features: Arc<PreviewFeatureStore>,
    ) -> Self {
        Self {
            storage,
            repository,
            locks,
            quota,
            features,
        }
    }

    /// 処理したジョブの件数を返す。
    pub async fn execute(&self, now_ms: u64) -> usize {
        let mut processed = 0;
        for expired in self.repository.list_expired(now_ms) {
            let lock = self.locks.lock_for(&expired.id);
            let _guard = lock.lock_owned().await;

            // ロックを待つ間に状態が変わったり消えたりしているかもしれないので、取り直して確かめる
            let Some(job) = self.repository.get(&expired.id) else {
                continue;
            };
            if job.deadline_ms() >= now_ms {
                continue;
            }

            if job.phase.is_in_progress() {
                finish_job(
                    VideoJob::clone(&job),
                    JobPhase::Failed(FailureCode::Timeout),
                    now_ms,
                    self.repository.as_ref(),
                    self.storage.as_ref(),
                    self.quota.as_ref(),
                )
                .await;
            } else {
                for key in [job.input_key(), job.output_key()] {
                    if let Err(e) = self.storage.delete(&key).await {
                        eprintln!("期限切れのファイルの削除に失敗 key={}: {:?}", key, e);
                    }
                }
                // 特徴量 → ストア → ロックの順に消す（ロックを持ったまま。待っている側は取り直して NotFound になる）
                self.features.remove(&job.id);
                self.repository.remove(&job.id);
                self.locks.remove(&job.id);
            }
            processed += 1;
        }
        // ジョブが無いのに残ったロック（存在確認とロック作成の間に消された場合など）を掃除する
        self.locks.retain_ids(|id| self.repository.get(id).is_some());
        self.features.retain_ids(|id| self.repository.get(id).is_some());
        processed
    }
}

#[cfg(test)]
mod delete_expired_video_jobs_usecase_test {
    use super::*;
    use crate::video_cutout::testing::*;

    #[tokio::test]
    async fn cleanup_deletes_files_of_finished_jobs_after_30min() {
        let h = started_job().await;
        h.report(done()).await;
        h.clock.advance(30 * 60_000 + 1);
        assert_eq!(h.cleanup().await, 1);
        assert!(h.storage.deleted().contains(&h.output_key()));
        assert!(h.storage.deleted().contains(&h.input_key()));
        assert!(h.repository.get(&h.id).is_none());
    }

    #[tokio::test]
    async fn cleanup_keeps_jobs_before_deadline() {
        let h = started_job().await;
        h.report(done()).await;
        h.clock.advance(30 * 60_000);
        assert_eq!(h.cleanup().await, 0);
        assert!(h.repository.get(&h.id).is_some());
    }

    #[tokio::test]
    async fn cleanup_deletes_expired_created_job_and_its_lock() {
        let h = harness();
        let id = h.created_job().await;
        let job = VideoJob::clone(&h.repository.get(&id).unwrap());
        let old_lock = h.locks.lock_for(&id);
        h.clock.advance(30 * 60_000 + 1);
        assert_eq!(h.cleanup.execute(h.clock_now()).await, 1);
        assert!(h.repository.get(&id).is_none());
        let deleted = h.storage.deleted();
        assert!(deleted.contains(&job.input_key()) && deleted.contains(&job.output_key()));
        // ロックも消えている（新しい Mutex が作られる）
        assert!(!Arc::ptr_eq(&old_lock, &h.locks.lock_for(&id)));
    }

    #[tokio::test]
    async fn cleanup_drops_preview_features_with_the_job() {
        let h = harness();
        let id = h.created_job().await;
        h.set_frame.execute(frame_input(id, png_bytes(4, 4))).await.unwrap();
        // ジョブが無いのに残った特徴量も消える
        let orphan = crate::video_cutout::job::id::VideoJobId::new_random();
        h.features.save(orphan, START_MS, h.features.get(&id, START_MS).unwrap());
        h.clock.advance(30 * 60_000);
        assert_eq!(h.cleanup.execute(h.clock_now()).await, 0);
        assert!(h.features.get(&id, START_MS).is_some());
        assert!(h.features.get(&orphan, START_MS).is_none());
        h.clock.advance(1);
        assert_eq!(h.cleanup.execute(h.clock_now()).await, 1);
        assert!(h.features.get(&id, START_MS).is_none());
        assert_eq!(h.features.len(), 0);
    }

    #[tokio::test]
    async fn cleanup_times_out_in_progress_job_then_deletes_it_30min_later() {
        let h = started_job().await;
        h.report(tracking(0.3)).await;
        h.clock.advance(3 * 60_000 + 1);
        assert_eq!(h.cleanup().await, 1);
        let job = h.repository.get(&h.id).unwrap();
        assert_eq!(job.phase, JobPhase::Failed(FailureCode::Timeout));
        assert!(job.quota_refunded);
        assert_eq!(job.last_event_at_ms, h.clock_now());
        assert_eq!(h.remaining().await, 3);
        assert_eq!(h.storage.deleted(), vec![h.input_key()]);

        h.clock.advance(30 * 60_000);
        assert_eq!(h.cleanup().await, 0);
        h.clock.advance(1);
        assert_eq!(h.cleanup().await, 1);
        assert!(h.repository.get(&h.id).is_none());
        assert!(h.storage.deleted().contains(&h.output_key()));
        // 二重に返却しない
        assert_eq!(h.remaining().await, 3);
    }

    #[tokio::test]
    async fn cleanup_deletes_failed_job_without_refunding_again() {
        let h = started_job().await;
        h.report(failed("subject_not_found")).await;
        assert_eq!(h.remaining().await, 3);
        h.clock.advance(30 * 60_000 + 1);
        assert_eq!(h.cleanup().await, 1);
        assert!(h.repository.get(&h.id).is_none());
        assert!(h.storage.deleted().contains(&h.output_key()));
        assert_eq!(h.remaining().await, 3);
    }

    #[tokio::test]
    async fn cleanup_removes_orphan_locks_and_keeps_live_ones() {
        let h = started_job().await;
        let orphan = crate::video_cutout::job::id::VideoJobId::new_random();
        h.locks.lock_for(&orphan);
        let live_before = h.locks.lock_for(&h.id);
        assert_eq!(h.locks.len(), 2);
        assert_eq!(h.cleanup().await, 0);
        assert_eq!(h.locks.len(), 1);
        // 生きているジョブのロックは同じ Mutex のまま
        assert!(Arc::ptr_eq(&live_before, &h.locks.lock_for(&h.id)));
    }

    #[tokio::test]
    async fn cleanup_continues_when_delete_fails() {
        let h = started_job().await;
        h.report(done()).await;
        h.storage.set_fail_delete(true);
        h.clock.advance(30 * 60_000 + 1);
        assert_eq!(h.cleanup().await, 1);
        assert!(h.repository.get(&h.id).is_none());
    }
}
