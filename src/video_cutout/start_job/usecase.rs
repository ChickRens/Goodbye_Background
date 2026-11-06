//! ジョブ開始（仕様 §4.5 `StartVideoJob`）。
//!
//! ジョブ単位のロックを持ったまま: 持ち主確認 → `Created` か → R2 に HEAD → 回数を消費 → `Queued` で保存 → ワーカーに投入。
//! - アップロードが無ければ `UPLOAD_NOT_FOUND`（回数は消費しない）。
//! - 実サイズが 200MB を超えていれば回数を消費せず `Failed(TooLarge)` にして `Ok` を返す。
//! - 投入に失敗したら回数を返して `Failed(DispatchFailed)` にし、`Ok` を返す（結果はポーリングで伝わる）。
//!
//! 回数の消費・返却は常にジョブの持ち主のキー（作成時の client_id と IP）で行う。
//! 後で返却するとき（期限切れ・ワーカーの失敗通知）はジョブしか手元に無いため、キーを揃えておく。

use std::sync::Arc;
use std::time::Duration;

use crate::video_cutout::{
    clock::VideoClock,
    job::{
        error::VideoJobError,
        locks::VideoJobLocks,
        phase::{FailureCode, JobPhase},
        repository::VideoJobRepository,
        video_job::{MAX_INPUT_BYTES, VideoJob},
    },
    quota::{
        daily_quota::{DailyQuota, QuotaKeys},
        jst_day::jst_day,
    },
    start_job::{
        error::{StartError, StartVideoJobError},
        input::StartVideoJobInput,
        output::StartVideoJobOutput,
    },
    storage::video_object_storage::VideoObjectStorage,
    worker::{
        cutout_worker::CutoutWorker,
        request::{WorkerJobRequest, WorkerPoint, WorkerRange},
    },
};

/// ワーカーに渡す入出力 URL の有効期間。
const WORKER_URL_TTL: Duration = Duration::from_secs(30 * 60);
/// MVP では常に透かしを入れる（仕様 D10）。
const WATERMARK: bool = true;

pub struct StartVideoJobUseCase {
    public_base_url: String,
    storage: Arc<dyn VideoObjectStorage>,
    worker: Arc<dyn CutoutWorker>,
    repository: Arc<dyn VideoJobRepository>,
    locks: Arc<VideoJobLocks>,
    quota: Arc<dyn DailyQuota>,
    clock: Arc<dyn VideoClock>,
}

impl StartVideoJobUseCase {
    pub fn new(
        public_base_url: String,
        storage: Arc<dyn VideoObjectStorage>,
        worker: Arc<dyn CutoutWorker>,
        repository: Arc<dyn VideoJobRepository>,
        locks: Arc<VideoJobLocks>,
        quota: Arc<dyn DailyQuota>,
        clock: Arc<dyn VideoClock>,
    ) -> Self {
        Self {
            public_base_url,
            storage,
            worker,
            repository,
            locks,
            quota,
            clock,
        }
    }

    pub async fn execute(
        &self,
        input: StartVideoJobInput,
    ) -> Result<StartVideoJobOutput, StartVideoJobError> {
        // HEAD とワーカー投入（非同期の待ち）をまたいで、取得 → 変更 → 保存を直列化する
        // 無い ID のロックを作らない（消す人がいないため）。ロック内でもう一度取り直す
        if self.repository.get(&input.job_id).is_none() {
            return Err(VideoJobError::NotFound.into());
        }
        let lock = self.locks.lock_for(&input.job_id);
        let _guard = lock.lock_owned().await;

        // 他人のジョブは存在しないものとして扱う
        let job = self
            .repository
            .get(&input.job_id)
            .filter(|job| job.owner_client_id == input.client_id)
            .ok_or(VideoJobError::NotFound)?;
        if job.phase != JobPhase::Created {
            return Err(VideoJobError::InvalidState.into());
        }

        let size = self
            .storage
            .head(&job.input_key())
            .await?
            .ok_or(StartError::UploadNotFound)?;

        let now_ms = self.clock.now_ms();
        let day = jst_day(now_ms);
        let keys = QuotaKeys {
            client_id: job.owner_client_id,
            ip: job.owner_ip,
        };
        let mut job = VideoJob::clone(&job);

        if size > MAX_INPUT_BYTES {
            // 申告より大きいファイルが置かれた。回数は消費しない
            self.fail(&mut job, FailureCode::TooLarge, now_ms).await;
            let remaining_today = self.quota.remaining(&keys, day).await?;
            return Ok(StartVideoJobOutput {
                phase: job.phase,
                remaining_today,
            });
        }

        let remaining_today = self.quota.try_consume(&keys, day).await?;
        job.phase = JobPhase::Queued;
        job.points = Some(input.points);
        job.background = Some(input.background);
        job.quota_consumed = true;
        job.last_event_at_ms = now_ms;
        self.repository.save(job.clone());

        let request = self.worker_request(&job);
        match self.worker.dispatch(&request).await {
            Ok(()) => Ok(StartVideoJobOutput {
                phase: job.phase,
                remaining_today,
            }),
            Err(e) => {
                println!("ワーカーへの投入に失敗 job={}: {:?}", job.id.value(), e);
                let refunded = match self.quota.refund(&keys, day).await {
                    Ok(()) => true,
                    Err(e) => {
                        println!("回数の返却に失敗 job={}: {:?}", job.id.value(), e);
                        false
                    }
                };
                job.quota_refunded = refunded;
                self.fail(&mut job, FailureCode::DispatchFailed, now_ms).await;
                // 返却は client / IP の両方を 1 減らすので、残り回数はちょうど 1 増える
                let remaining_today = if refunded {
                    remaining_today + 1
                } else {
                    remaining_today
                };
                Ok(StartVideoJobOutput {
                    phase: job.phase,
                    remaining_today,
                })
            }
        }
    }

    /// `Failed(code)` にして保存し、入力ファイルを削除する（削除の失敗はログだけ。仕様 §5.3）。
    async fn fail(&self, job: &mut VideoJob, code: FailureCode, now_ms: u64) {
        job.phase = JobPhase::Failed(code);
        job.last_event_at_ms = now_ms;
        self.repository.save(job.clone());
        if let Err(e) = self.storage.delete(&job.input_key()).await {
            println!("入力ファイルの削除に失敗 job={}: {:?}", job.id.value(), e);
        }
    }

    fn worker_request(&self, job: &VideoJob) -> WorkerJobRequest {
        let id = job.id.value().to_string();
        WorkerJobRequest {
            input_url: self
                .storage
                .presign_get(&job.input_key(), WORKER_URL_TTL, None),
            output_url: self
                .storage
                .presign_put(&job.output_key(), "video/mp4", WORKER_URL_TTL),
            range_ms: WorkerRange {
                start: job.range.start_ms(),
                end: job.range.end_ms(),
            },
            points: job
                .points
                .as_ref()
                .map(|p| p.as_slice().iter().map(WorkerPoint::from).collect())
                .unwrap_or_default(),
            background_rgb: job.background.map(|b| b.rgb()).unwrap_or([0, 255, 0]),
            watermark: WATERMARK,
            max_input_bytes: MAX_INPUT_BYTES,
            callback_url: format!(
                "{}/internal/video-jobs/{}/events",
                self.public_base_url.trim_end_matches('/'),
                id
            ),
            job_id: id,
        }
    }
}

#[cfg(test)]
mod start_video_job_usecase_test {
    use super::*;
    use crate::video_cutout::{job::id::VideoJobId, quota::error::QuotaError, testing::*};
    use uuid::Uuid;

    #[tokio::test]
    async fn start_queues_and_dispatches_request() {
        let h = harness();
        let id = h.created_job().await;
        let out = h.start.execute(start_input(id)).await.unwrap();
        assert_eq!(out.remaining_today, 2);
        assert_eq!(out.phase, JobPhase::Queued);
        assert_eq!(h.remaining().await, 2);

        let job = h.repository.get(&id).unwrap();
        assert_eq!(job.phase, JobPhase::Queued);
        assert!(job.quota_consumed && !job.quota_refunded);
        assert_eq!(job.points, Some(start_input(id).points));
        assert_eq!(job.background, Some(start_input(id).background));

        let sent = h.worker.dispatched.lock().unwrap();
        assert_eq!(sent.len(), 1);
        let req = &sent[0];
        let id_str = id.value().to_string();
        assert_eq!(req.job_id, id_str);
        assert!(req.input_url.contains(&job.input_key()) && req.input_url.contains("method=GET"));
        assert!(req.input_url.contains("X-Amz-Expires=1800"));
        assert!(req.output_url.contains(&job.output_key()) && req.output_url.contains("method=PUT"));
        assert!(req.output_url.contains("X-Amz-Expires=1800"));
        assert_eq!(req.range_ms, WorkerRange { start: 1_000, end: 9_000 });
        assert_eq!(req.points.len(), 1);
        assert_eq!(req.background_rgb, [0, 255, 0]);
        assert!(req.watermark);
        assert_eq!(req.max_input_bytes, 209_715_200);
        assert_eq!(
            req.callback_url,
            format!("https://example.test/internal/video-jobs/{id_str}/events")
        );
    }

    #[tokio::test]
    async fn start_dispatches_once_under_concurrency() {
        let h = harness_with(FakeWorker::delayed(Duration::from_millis(50)));
        let id = h.created_job().await;
        let (a, b) = tokio::join!(h.start.execute(start_input(id)), h.start.execute(start_input(id)));
        assert_eq!([a.is_ok(), b.is_ok()].iter().filter(|x| **x).count(), 1);
        assert_eq!(h.worker.dispatched.lock().unwrap().len(), 1);
        assert_eq!(h.remaining().await, 2);
    }

    #[tokio::test]
    async fn start_twice_is_invalid_state() {
        let h = harness();
        let id = h.created_job().await;
        h.start.execute(start_input(id)).await.unwrap();
        assert!(matches!(
            h.start.execute(start_input(id)).await,
            Err(StartVideoJobError::Job(VideoJobError::InvalidState))
        ));
    }

    #[tokio::test]
    async fn start_before_upload_is_conflict_and_keeps_quota() {
        let h = harness_with_head(None);
        let id = h.created_job().await;
        assert!(matches!(
            h.start.execute(start_input(id)).await,
            Err(StartVideoJobError::Start(StartError::UploadNotFound))
        ));
        assert_eq!(h.remaining().await, 3);
        assert_eq!(h.phase(id), JobPhase::Created);
    }

    #[tokio::test]
    async fn second_job_exceeds_quota_and_stays_created() {
        let h = harness_with_used(2);
        let a = h.created_job().await;
        let b = h.created_job().await;
        h.start.execute(start_input(a)).await.unwrap();
        assert!(matches!(
            h.start.execute(start_input(b)).await,
            Err(StartVideoJobError::Quota(QuotaError::Exceeded(_)))
        ));
        assert_eq!(h.phase(b), JobPhase::Created);
        assert_eq!(h.worker.dispatched.lock().unwrap().len(), 1);
    }

    #[tokio::test]
    async fn dispatch_failure_refunds() {
        let h = harness_with(FakeWorker::failing());
        let id = h.created_job().await;
        let out = h.start.execute(start_input(id)).await.unwrap();
        assert_eq!(h.phase(id), JobPhase::Failed(FailureCode::DispatchFailed));
        assert_eq!(h.remaining().await, 3);
        assert_eq!(out.phase, JobPhase::Failed(FailureCode::DispatchFailed));
        assert_eq!(out.remaining_today, 3);
        let job = h.repository.get(&id).unwrap();
        assert!(job.quota_consumed && job.quota_refunded);
        assert_eq!(*h.storage.deleted.lock().unwrap(), vec![job.input_key()]);
    }

    #[tokio::test]
    async fn other_client_gets_not_found() {
        let h = harness();
        let id = h.created_job().await;
        assert!(matches!(
            h.start.execute(StartVideoJobInput { client_id: Uuid::new_v4(), ..start_input(id) }).await,
            Err(StartVideoJobError::Job(VideoJobError::NotFound))
        ));
        assert!(matches!(
            h.start.execute(start_input(VideoJobId::new_random())).await,
            Err(StartVideoJobError::Job(VideoJobError::NotFound))
        ));
        assert_eq!(h.phase(id), JobPhase::Created);
    }

    #[tokio::test]
    async fn unknown_job_does_not_create_lock() {
        let h = harness();
        assert!(matches!(
            h.start.execute(start_input(VideoJobId::new_random())).await,
            Err(StartVideoJobError::Job(VideoJobError::NotFound))
        ));
        assert_eq!(h.locks.len(), 0);
    }

    #[tokio::test]
    async fn oversized_upload_fails_without_consuming() {
        let h = harness_with_head(Some(209_715_201));
        let id = h.created_job().await;
        let out = h.start.execute(start_input(id)).await.unwrap();
        assert_eq!(h.phase(id), JobPhase::Failed(FailureCode::TooLarge));
        assert_eq!(h.remaining().await, 3);
        assert_eq!(out.phase, JobPhase::Failed(FailureCode::TooLarge));
        assert_eq!(out.remaining_today, 3);
        assert!(h.worker.dispatched.lock().unwrap().is_empty());
        let job = h.repository.get(&id).unwrap();
        assert!(!job.quota_consumed);
        assert_eq!(*h.storage.deleted.lock().unwrap(), vec![job.input_key()]);
    }

    #[tokio::test]
    async fn failed_transition_records_event_time() {
        let h = harness_with(FakeWorker::failing());
        let id = h.created_job().await;
        h.clock.advance(5_000);
        h.start.execute(start_input(id)).await.unwrap();
        assert_eq!(h.repository.get(&id).unwrap().last_event_at_ms, NOW_MS + 5_000);
    }
}
