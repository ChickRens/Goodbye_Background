//! 動画ジョブ本体。ストアには丸ごと置き換える形で保存する（clone して変更 → 保存）。

use std::net::IpAddr;

use uuid::Uuid;

use crate::video_cutout::job::{
    background::Background, clip_range::ClipRange, id::VideoJobId, phase::JobPhase,
    point::Points,
};

const MINUTE_MS: u64 = 60_000;

#[derive(Debug, PartialEq, Clone, Copy)]
pub struct JobResult {
    pub frames: u32,
    pub lost_frames: u32,
    pub duration_ms: u32,
}

#[derive(Debug, PartialEq, Clone)]
pub struct VideoJob {
    pub id: VideoJobId,
    pub owner_client_id: Uuid,
    pub owner_ip: IpAddr,
    pub size_bytes: u64,
    pub content_type: String,
    pub range: ClipRange,
    pub background: Option<Background>,
    pub points: Option<Points>,
    pub phase: JobPhase,
    pub created_at_ms: u64,
    pub last_event_at_ms: u64,
    pub quota_consumed: bool,
    pub quota_refunded: bool,
    pub result: Option<JobResult>,
}

impl VideoJob {
    /// 入力動画のオブジェクトストレージ上のキー。
    pub fn input_key(&self) -> String {
        format!("input/{}", self.id.value())
    }

    /// 出力動画のオブジェクトストレージ上のキー。
    pub fn output_key(&self) -> String {
        format!("output/{}", self.id.value())
    }

    /// この時刻を過ぎたらジョブを期限切れとして扱う（仕様 §5.2）。
    pub fn deadline_ms(&self) -> u64 {
        match self.phase {
            JobPhase::Created => self.created_at_ms + 30 * MINUTE_MS,
            JobPhase::Queued => self.last_event_at_ms + 5 * MINUTE_MS,
            JobPhase::Downloading | JobPhase::Tracking(_) | JobPhase::Finishing => {
                self.last_event_at_ms + 3 * MINUTE_MS
            }
            JobPhase::Done | JobPhase::Failed(_) => self.last_event_at_ms + 30 * MINUTE_MS,
        }
    }
}

#[cfg(test)]
mod video_job_test {
    use super::*;
    use crate::video_cutout::job::phase::FailureCode;

    fn job_created_at(ms: u64) -> VideoJob {
        VideoJob {
            id: VideoJobId::new_random(),
            owner_client_id: Uuid::new_v4(),
            owner_ip: "127.0.0.1".parse().unwrap(),
            size_bytes: 1,
            content_type: "video/mp4".to_string(),
            range: ClipRange::new(0, 1_000).unwrap(),
            background: None,
            points: None,
            phase: JobPhase::Created,
            created_at_ms: ms,
            last_event_at_ms: ms,
            quota_consumed: false,
            quota_refunded: false,
            result: None,
        }
    }

    fn with_phase(mut job: VideoJob, phase: JobPhase, at_ms: u64) -> VideoJob {
        job.phase = phase;
        job.last_event_at_ms = at_ms;
        job
    }

    #[test]
    fn deadlines() {
        let j = job_created_at(1_000);
        assert_eq!(j.deadline_ms(), 1_000 + 30 * 60_000);
        assert_eq!(with_phase(j.clone(), JobPhase::Queued, 2_000).deadline_ms(), 2_000 + 5 * 60_000);
        assert_eq!(with_phase(j.clone(), JobPhase::Downloading, 2_500).deadline_ms(), 2_500 + 3 * 60_000);
        assert_eq!(with_phase(j.clone(), JobPhase::Tracking(0.1), 3_000).deadline_ms(), 3_000 + 3 * 60_000);
        assert_eq!(with_phase(j.clone(), JobPhase::Finishing, 3_500).deadline_ms(), 3_500 + 3 * 60_000);
        assert_eq!(with_phase(j.clone(), JobPhase::Failed(FailureCode::Timeout), 4_500).deadline_ms(), 4_500 + 30 * 60_000);
        assert_eq!(with_phase(j, JobPhase::Done, 4_000).deadline_ms(), 4_000 + 30 * 60_000);
    }

    #[test]
    fn keys_are_derived_from_id() {
        let j = job_created_at(0);
        assert_eq!(j.input_key(), format!("input/{}", j.id.value()));
        assert_eq!(j.output_key(), format!("output/{}", j.id.value()));
    }
}
