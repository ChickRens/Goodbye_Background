//! ジョブ状態の取得（仕様 §4.5 `GetVideoJob`）。
//!
//! ジョブ単位のロックを持ったまま: 持ち主確認 → 進行中で期限（§5.2）を過ぎていれば `Failed(timeout)` にして回数を返す
//! → `Done` なら 10 分有効の GET URL を発行。残り回数はジョブの持ち主のキーで数える。

use std::sync::Arc;
use std::time::Duration;

use crate::video_cutout::{
    clock::VideoClock,
    finish::finish_job,
    get_job::{error::GetVideoJobError, input::GetVideoJobInput, output::GetVideoJobOutput},
    job::{
        error::VideoJobError,
        locks::VideoJobLocks,
        phase::{FailureCode, JobPhase},
        repository::VideoJobRepository,
        video_job::VideoJob,
    },
    quota::{
        daily_quota::{DailyQuota, QuotaKeys},
        jst_day::jst_day,
    },
    storage::video_object_storage::VideoObjectStorage,
};

/// ダウンロード用 URL の有効期間。
const DOWNLOAD_URL_TTL: Duration = Duration::from_secs(10 * 60);

pub struct GetVideoJobUseCase {
    storage: Arc<dyn VideoObjectStorage>,
    repository: Arc<dyn VideoJobRepository>,
    locks: Arc<VideoJobLocks>,
    quota: Arc<dyn DailyQuota>,
    clock: Arc<dyn VideoClock>,
}

impl GetVideoJobUseCase {
    pub fn new(
        storage: Arc<dyn VideoObjectStorage>,
        repository: Arc<dyn VideoJobRepository>,
        locks: Arc<VideoJobLocks>,
        quota: Arc<dyn DailyQuota>,
        clock: Arc<dyn VideoClock>,
    ) -> Self {
        Self {
            storage,
            repository,
            locks,
            quota,
            clock,
        }
    }

    pub async fn execute(
        &self,
        input: GetVideoJobInput,
    ) -> Result<GetVideoJobOutput, GetVideoJobError> {
        let lock = self.locks.lock_for(&input.job_id);
        let _guard = lock.lock_owned().await;

        // 他人のジョブは存在しないものとして扱う
        let job = self
            .repository
            .get(&input.job_id)
            .filter(|job| job.owner_client_id == input.client_id)
            .ok_or(VideoJobError::NotFound)?;

        let now_ms = self.clock.now_ms();
        let mut job = VideoJob::clone(&job);
        if is_in_progress(&job.phase) && job.deadline_ms() < now_ms {
            job = finish_job(
                job,
                JobPhase::Failed(FailureCode::Timeout),
                now_ms,
                self.repository.as_ref(),
                self.storage.as_ref(),
                self.quota.as_ref(),
            )
            .await;
        }

        let keys = QuotaKeys {
            client_id: job.owner_client_id,
            ip: job.owner_ip,
        };
        let remaining_today = self.quota.remaining(&keys, jst_day(now_ms)).await?;

        let failure = match job.phase {
            JobPhase::Failed(code) => Some(code),
            _ => None,
        };
        let download_url = (job.phase == JobPhase::Done).then(|| {
            let id = job.id.value().to_string();
            let filename = format!("cutout-{}.mp4", &id[..8]);
            self.storage
                .presign_get(&job.output_key(), DOWNLOAD_URL_TTL, Some(&filename))
        });

        Ok(GetVideoJobOutput {
            phase: job.phase,
            failure,
            quota_refunded: job.quota_refunded,
            download_url,
            remaining_today,
        })
    }
}

/// 進行中（`Queued`〜`Finishing`）か。期限を過ぎたら `Failed(timeout)` にする対象。
pub fn is_in_progress(phase: &JobPhase) -> bool {
    !matches!(phase, JobPhase::Created) && !phase.is_terminal()
}

#[cfg(test)]
mod get_video_job_usecase_test {
    use super::*;
    use crate::video_cutout::{job::id::VideoJobId, testing::*};
    use serde_json::json;
    use uuid::Uuid;

    #[tokio::test]
    async fn tracking_progress_and_done_gives_download_url() {
        let h = started_job().await;
        h.report(json!({"job_id": h.id_str(), "phase": "tracking", "progress": 0.5}))
            .await;
        assert_eq!(h.get().await.phase, JobPhase::Tracking(0.5));
        h.report(json!({"job_id": h.id_str(), "phase": "done", "frames": 30, "lost_frames": 0, "duration_ms": 1000})).await;
        let out = h.get().await;
        assert_eq!(out.phase, JobPhase::Done);
        let url = out.download_url.unwrap();
        assert!(url.contains("output.mp4"));
        assert!(url.contains("method=GET") && url.contains("X-Amz-Expires=600"));
        assert!(url.contains(&format!("filename=cutout-{}.mp4", &h.id_str()[..8])));
        assert_eq!(out.failure, None);
        assert_eq!(out.remaining_today, 2);
        assert_eq!(h.storage.deleted(), vec![h.input_key()]);
    }

    #[tokio::test]
    async fn in_progress_has_no_url_or_failure() {
        let h = started_job().await;
        let out = h.get().await;
        assert_eq!(out.phase, JobPhase::Queued);
        assert_eq!(
            (out.failure, out.download_url, out.quota_refunded),
            (None, None, false)
        );
    }

    #[tokio::test]
    async fn get_times_out_queued_after_5min() {
        let h = started_job().await;
        h.clock.advance(5 * 60_000 + 1);
        let out = h.get().await;
        assert_eq!(out.phase, JobPhase::Failed(FailureCode::Timeout));
        assert_eq!(out.failure, Some(FailureCode::Timeout));
        assert!(out.quota_refunded);
        assert_eq!(out.remaining_today, 3);
        assert_eq!(h.remaining().await, 3);
        assert_eq!(h.storage.deleted(), vec![h.input_key()]);
        let job = h.repository.get(&h.id).unwrap();
        assert_eq!(job.last_event_at_ms, NOW_MS + 5 * 60_000 + 1);
        // もう一度取得しても二重に返却しない
        h.get().await;
        assert_eq!(h.remaining().await, 3);
    }

    #[tokio::test]
    async fn get_keeps_queued_until_deadline() {
        let h = started_job().await;
        h.clock.advance(5 * 60_000);
        assert_eq!(h.get().await.phase, JobPhase::Queued);
    }

    #[tokio::test]
    async fn get_times_out_tracking_after_3min_since_last_event() {
        let h = started_job().await;
        h.clock.advance(60_000);
        h.report(tracking(0.2)).await;
        h.clock.advance(3 * 60_000);
        assert_eq!(h.get().await.phase, JobPhase::Tracking(0.2));
        h.clock.advance(1);
        assert_eq!(h.get().await.phase, JobPhase::Failed(FailureCode::Timeout));
    }

    #[tokio::test]
    async fn expired_created_job_is_left_for_cleanup() {
        let h = harness();
        let id = h.created_job().await;
        h.clock.advance(30 * 60_000 + 1);
        let out = h.get.execute(get_input(id)).await.unwrap();
        assert_eq!(out.phase, JobPhase::Created);
        assert!(h.storage.deleted().is_empty());
    }

    #[tokio::test]
    async fn other_client_or_unknown_job_is_not_found() {
        let h = started_job().await;
        assert!(matches!(
            h.get
                .execute(GetVideoJobInput {
                    client_id: Uuid::new_v4(),
                    ..get_input(h.id)
                })
                .await,
            Err(GetVideoJobError::Job(VideoJobError::NotFound))
        ));
        assert!(matches!(
            h.get.execute(get_input(VideoJobId::new_random())).await,
            Err(GetVideoJobError::Job(VideoJobError::NotFound))
        ));
    }

    #[tokio::test]
    async fn timeout_of_unconsumed_job_does_not_refund() {
        let h = harness();
        let id = h.created_job().await;
        // 回数を消費していない進行中のジョブ（通常は起きないが、返却の条件を確かめる）
        let mut job = VideoJob::clone(&h.repository.get(&id).unwrap());
        job.phase = JobPhase::Queued;
        h.repository.save(job);
        h.clock.advance(5 * 60_000 + 1);
        let out = h.get.execute(get_input(id)).await.unwrap();
        assert_eq!(out.phase, JobPhase::Failed(FailureCode::Timeout));
        assert!(!out.quota_refunded);
        assert_eq!(h.remaining().await, 3);
    }
}
