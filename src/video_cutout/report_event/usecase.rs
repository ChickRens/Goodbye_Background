//! ワーカー通知の反映（仕様 §4.5 `ReportVideoJobEvent`）。
//!
//! ジョブ単位のロックを持ったまま状態を進める。進行中のまま期限（§5.2）を過ぎていれば、通知の内容にかかわらず
//! `Failed(timeout)` にする（取得・期限処理のどちらが先に走っても結果が同じになるように）。前に進まない通知（順番の入れ替わり・最終状態の後に届いたもの）は無視する。
//! `Done` / `Failed` になったら入力ファイルを削除し、`Failed` は仕様 §5.1 の表に従って回数を返す（`finish_job`）。

use std::sync::Arc;

use crate::video_cutout::{
    clock::VideoClock,
    finish::finish_job,
    job::{
        error::VideoJobError,
        locks::VideoJobLocks,
        phase::{FailureCode, JobPhase},
        repository::VideoJobRepository,
        video_job::VideoJob,
    },
    quota::daily_quota::DailyQuota,
    report_event::{
        error::ReportVideoJobEventError, input::ReportVideoJobEventInput,
        output::ReportVideoJobEventOutput,
    },
    storage::video_object_storage::VideoObjectStorage,
};

pub struct ReportVideoJobEventUseCase {
    storage: Arc<dyn VideoObjectStorage>,
    repository: Arc<dyn VideoJobRepository>,
    locks: Arc<VideoJobLocks>,
    quota: Arc<dyn DailyQuota>,
    clock: Arc<dyn VideoClock>,
}

impl ReportVideoJobEventUseCase {
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
        input: ReportVideoJobEventInput,
    ) -> Result<ReportVideoJobEventOutput, ReportVideoJobEventError> {
        // 無い ID のロックを作らない（消す人がいないため）。ロック内でもう一度取り直す
        if self.repository.get(&input.job_id).is_none() {
            return Err(VideoJobError::NotFound.into());
        }
        let lock = self.locks.lock_for(&input.job_id);
        let _guard = lock.lock_owned().await;

        let job = self
            .repository
            .get(&input.job_id)
            .ok_or(VideoJobError::NotFound)?;
        let now_ms = self.clock.now_ms();
        // 期限を過ぎた進行中のジョブは、通知より先にタイムアウトさせる（取得・期限処理と結果を揃えるため）
        if job.phase.is_in_progress() && job.deadline_ms() < now_ms {
            finish_job(
                VideoJob::clone(&job),
                JobPhase::Failed(FailureCode::Timeout),
                now_ms,
                self.repository.as_ref(),
                self.storage.as_ref(),
                self.quota.as_ref(),
            )
            .await;
            return Ok(());
        }
        // 前に進まない通知（順番の入れ替わり、最終状態の後に遅れて届いたもの）は何もしない
        let Some(next) = job.phase.advance(input.event.phase.to_job_phase()) else {
            return Ok(());
        };

        let mut job = VideoJob::clone(&job);
        if next.is_terminal() {
            job.result = input.event.phase.result();
            finish_job(
                job,
                next,
                now_ms,
                self.repository.as_ref(),
                self.storage.as_ref(),
                self.quota.as_ref(),
            )
            .await;
        } else {
            job.phase = next;
            job.last_event_at_ms = now_ms;
            self.repository.save(job);
        }
        Ok(())
    }
}

#[cfg(test)]
mod report_video_job_event_usecase_test {
    use super::*;
    use crate::video_cutout::{
        job::{
            id::VideoJobId,
            phase::{FailureCode, JobPhase},
            video_job::JobResult,
        },
        report_event::input::{WorkerEvent, WorkerEventPhase},
        testing::*,
    };

    #[tokio::test]
    async fn out_of_order_and_late_events_are_ignored() {
        let h = started_job().await;
        h.report(tracking(0.6)).await;
        h.report(tracking(0.4)).await;
        assert_eq!(h.get().await.phase, JobPhase::Tracking(0.6));
        h.report(failed("internal_error")).await;
        h.report(done()).await;
        assert_eq!(
            h.get().await.phase,
            JobPhase::Failed(FailureCode::InternalError)
        );
        assert!(h.repository.get(&h.id).unwrap().result.is_none());
    }

    #[tokio::test]
    async fn failure_refund_follows_table() {
        for (code, refunded) in [
            ("unsupported_video", true),
            ("subject_not_found", true),
            ("internal_error", true),
            ("range_out_of_bounds", false),
            ("too_large", false),
        ] {
            let h = started_job().await;
            h.report(failed(code)).await;
            assert_eq!(
                (h.get().await.quota_refunded, h.remaining().await == 3),
                (refunded, refunded),
                "{code}"
            );
            assert_eq!(h.storage.deleted(), vec![h.input_key()], "{code}");
        }
    }

    #[tokio::test]
    async fn unknown_failure_code_is_internal_error_and_refunds() {
        let h = started_job().await;
        h.report(failed("gpu_on_fire")).await;
        let out = h.get().await;
        assert_eq!(out.phase, JobPhase::Failed(FailureCode::InternalError));
        assert!(out.quota_refunded);
    }

    #[tokio::test]
    async fn late_events_after_timeout_do_nothing() {
        let h = started_job().await;
        h.clock.advance(5 * 60_000 + 1);
        assert_eq!(h.get().await.phase, JobPhase::Failed(FailureCode::Timeout));
        let before = VideoJob::clone(&h.repository.get(&h.id).unwrap());
        h.clock.advance(1_000);
        h.report(failed("internal_error")).await;
        h.report(done()).await;
        assert_eq!(*h.repository.get(&h.id).unwrap(), before);
        assert_eq!(h.remaining().await, 3);
        assert_eq!(h.storage.deleted(), vec![h.input_key()]);
    }

    #[tokio::test]
    async fn done_records_result_and_event_time() {
        let h = started_job().await;
        h.clock.advance(2_000);
        h.report(done()).await;
        let job = h.repository.get(&h.id).unwrap();
        assert_eq!(
            job.result,
            Some(JobResult {
                frames: 30,
                lost_frames: 0,
                duration_ms: 1000
            })
        );
        assert_eq!(job.last_event_at_ms, NOW_MS + 2_000);
        assert!(!job.quota_refunded);
        assert_eq!(h.remaining().await, 2);
    }

    #[tokio::test]
    async fn progress_updates_event_time_and_keeps_input() {
        let h = started_job().await;
        h.clock.advance(1_000);
        h.report(tracking(0.1)).await;
        let job = h.repository.get(&h.id).unwrap();
        assert_eq!(job.last_event_at_ms, NOW_MS + 1_000);
        assert!(h.storage.deleted().is_empty());
    }

    #[tokio::test]
    async fn event_after_deadline_times_out_instead_of_advancing() {
        let h = started_job().await;
        h.clock.advance(5 * 60_000 + 1);
        h.report(done()).await;
        let job = h.repository.get(&h.id).unwrap();
        assert_eq!(job.phase, JobPhase::Failed(FailureCode::Timeout));
        assert!(job.quota_refunded && job.result.is_none());
        assert_eq!(job.last_event_at_ms, h.clock_now());
        assert_eq!(h.remaining().await, 3);
        assert_eq!(h.storage.deleted(), vec![h.input_key()]);
    }

    #[tokio::test]
    async fn event_just_at_deadline_still_advances() {
        let h = started_job().await;
        h.clock.advance(5 * 60_000);
        h.report(tracking(0.1)).await;
        assert_eq!(h.repository.get(&h.id).unwrap().phase, JobPhase::Tracking(0.1));
    }

    #[tokio::test]
    async fn unknown_job_is_not_found() {
        let h = started_job().await;
        let input = ReportVideoJobEventInput {
            job_id: VideoJobId::new_random(),
            event: WorkerEvent {
                job_id: String::new(),
                phase: WorkerEventPhase::Finishing,
            },
        };
        assert!(matches!(
            h.report_uc.execute(input).await,
            Err(ReportVideoJobEventError::Job(VideoJobError::NotFound))
        ));
        // 開始済みジョブのロックだけが残り、無い ID のロックは作られない
        assert_eq!(h.locks.len(), 1);
    }
}
