//! ジョブ作成（仕様 §4.5 `CreateVideoJob`）。
//!
//! 入力を検証し、残り回数 > 0 を確かめて（消費はしない）`Created` で保存し、15 分有効の PUT URL を返す。

use std::sync::Arc;
use std::time::Duration;

use crate::video_cutout::{
    clock::VideoClock,
    create_job::{
        error::{CreateError, CreateVideoJobError},
        input::CreateVideoJobInput,
        output::CreateVideoJobOutput,
    },
    job::{
        clip_range::ClipRange,
        id::VideoJobId,
        phase::JobPhase,
        repository::VideoJobRepository,
        video_job::{MAX_INPUT_BYTES, VideoJob},
    },
    quota::{
        daily_quota::{DailyQuota, QuotaKeys},
        error::QuotaError,
        jst_day::jst_day,
    },
    storage::video_object_storage::VideoObjectStorage,
};

/// アップロード用 URL の有効期間。
const UPLOAD_URL_TTL: Duration = Duration::from_secs(15 * 60);
/// 受け付ける Content-Type。
const ALLOWED_CONTENT_TYPES: [&str; 2] = ["video/mp4", "video/quicktime"];

pub struct CreateVideoJobUseCase {
    storage: Arc<dyn VideoObjectStorage>,
    repository: Arc<dyn VideoJobRepository>,
    quota: Arc<dyn DailyQuota>,
    clock: Arc<dyn VideoClock>,
}

impl CreateVideoJobUseCase {
    pub fn new(
        storage: Arc<dyn VideoObjectStorage>,
        repository: Arc<dyn VideoJobRepository>,
        quota: Arc<dyn DailyQuota>,
        clock: Arc<dyn VideoClock>,
    ) -> Self {
        Self {
            storage,
            repository,
            quota,
            clock,
        }
    }

    pub async fn execute(
        &self,
        input: CreateVideoJobInput,
    ) -> Result<CreateVideoJobOutput, CreateVideoJobError> {
        if input.size_bytes > MAX_INPUT_BYTES {
            return Err(CreateError::TooLarge.into());
        }
        if !ALLOWED_CONTENT_TYPES.contains(&input.content_type.as_str()) {
            return Err(CreateError::UnsupportedContentType.into());
        }
        let range = ClipRange::new(input.start_ms, input.end_ms)?;

        let now_ms = self.clock.now_ms();
        let keys = QuotaKeys {
            client_id: input.client_id,
            ip: input.ip,
        };
        let remaining_today = self.quota.remaining(&keys, jst_day(now_ms)).await?;
        if remaining_today == 0 {
            return Err(QuotaError::Exceeded("残り回数が 0".to_string()).into());
        }

        let job = VideoJob {
            id: VideoJobId::new_random(),
            owner_client_id: input.client_id,
            owner_ip: input.ip,
            size_bytes: input.size_bytes,
            content_type: input.content_type,
            range,
            background: None,
            points: None,
            phase: JobPhase::Created,
            created_at_ms: now_ms,
            last_event_at_ms: now_ms,
            quota_consumed: false,
            quota_refunded: false,
            result: None,
        };
        let upload_url =
            self.storage
                .presign_put(&job.input_key(), &job.content_type, UPLOAD_URL_TTL);
        let job_id = job.id;
        self.repository.save(job);

        Ok(CreateVideoJobOutput {
            job_id,
            upload_url,
            remaining_today,
        })
    }
}

#[cfg(test)]
mod create_video_job_usecase_test {
    use super::*;
    use crate::video_cutout::{job::error::VideoJobError, testing::*};

    #[tokio::test]
    async fn create_checks_quota_without_consuming() {
        let h = harness();
        let out = h.create.execute(create_input()).await.unwrap();
        assert_eq!(out.remaining_today, 3);
        assert!(out.upload_url.contains("X-Amz-Expires=900"));
        assert_eq!(h.remaining().await, 3);
    }

    #[tokio::test]
    async fn create_saves_created_job_and_signs_input_key() {
        let h = harness();
        let out = h.create.execute(create_input()).await.unwrap();
        let job = h.repository.get(&out.job_id).unwrap();
        assert_eq!(job.phase, JobPhase::Created);
        assert_eq!(job.owner_client_id, client_id());
        assert_eq!(job.owner_ip, client_ip());
        assert_eq!(job.size_bytes, 10_000_000);
        assert_eq!(job.content_type, "video/mp4");
        assert_eq!(job.range, ClipRange::new(1_000, 9_000).unwrap());
        assert_eq!(job.created_at_ms, NOW_MS);
        assert!(!job.quota_consumed);
        assert!(out.upload_url.contains(&job.input_key()));
        assert!(out.upload_url.contains("content-type=video/mp4"));
    }

    #[tokio::test]
    async fn create_rejects_size_and_type() {
        let h = harness();
        assert!(matches!(
            h.create.execute(CreateVideoJobInput { size_bytes: 209_715_201, ..create_input() }).await,
            Err(CreateVideoJobError::Create(CreateError::TooLarge))
        ));
        assert!(matches!(
            h.create.execute(CreateVideoJobInput { content_type: "video/webm".into(), ..create_input() }).await,
            Err(CreateVideoJobError::Create(CreateError::UnsupportedContentType))
        ));
        // ちょうど 200MB と video/quicktime は受け付ける
        assert!(h.create.execute(CreateVideoJobInput {
            size_bytes: 209_715_200,
            content_type: "video/quicktime".into(),
            ..create_input()
        }).await.is_ok());
    }

    #[tokio::test]
    async fn create_rejects_bad_range() {
        let h = harness();
        assert!(matches!(
            h.create.execute(CreateVideoJobInput { start_ms: 0, end_ms: 10_001, ..create_input() }).await,
            Err(CreateVideoJobError::Job(VideoJobError::InvalidRange))
        ));
    }

    #[tokio::test]
    async fn create_rejects_when_no_quota_left() {
        let h = harness_with_used(3);
        assert!(matches!(
            h.create.execute(create_input()).await,
            Err(CreateVideoJobError::Quota(QuotaError::Exceeded(_)))
        ));
    }
}
