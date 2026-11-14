//! ワーカー通知の反映の入力と、ワーカーが送る通知の JSON（`contracts/video_cutout/worker_event_*.json`）。

use serde::Deserialize;

use crate::video_cutout::job::{
    id::VideoJobId,
    phase::{FailureCode, JobPhase},
    video_job::JobResult,
};

/// ワーカーが送る通知。`job_id` とパスの ID の照合は HTTP 層で行う。
#[derive(Debug, PartialEq, Clone, Deserialize)]
pub struct WorkerEvent {
    pub job_id: String,
    #[serde(flatten)]
    pub phase: WorkerEventPhase,
}

#[derive(Debug, PartialEq, Clone, Deserialize)]
#[serde(tag = "phase", rename_all = "lowercase")]
pub enum WorkerEventPhase {
    Downloading,
    Tracking {
        progress: f32,
    },
    Finishing,
    Done {
        frames: u32,
        lost_frames: u32,
        duration_ms: u32,
    },
    Failed {
        code: String,
    },
}

impl WorkerEventPhase {
    /// ジョブの状態に直す。未知の失敗コードは `InternalError` として扱う。
    pub fn to_job_phase(&self) -> JobPhase {
        match self {
            WorkerEventPhase::Downloading => JobPhase::Downloading,
            WorkerEventPhase::Tracking { progress } => JobPhase::Tracking(*progress),
            WorkerEventPhase::Finishing => JobPhase::Finishing,
            WorkerEventPhase::Done { .. } => JobPhase::Done,
            WorkerEventPhase::Failed { code } => JobPhase::Failed(
                FailureCode::from_worker(code).unwrap_or(FailureCode::InternalError),
            ),
        }
    }

    /// `Done` のときだけ結果の数値。
    pub fn result(&self) -> Option<JobResult> {
        match *self {
            WorkerEventPhase::Done {
                frames,
                lost_frames,
                duration_ms,
            } => Some(JobResult {
                frames,
                lost_frames,
                duration_ms,
            }),
            _ => None,
        }
    }
}

pub struct ReportVideoJobEventInput {
    pub job_id: VideoJobId,
    pub event: WorkerEvent,
}

#[cfg(test)]
mod worker_event_test {
    use super::*;

    const CONTRACTS: [&str; 5] = [
        include_str!("../../../contracts/video_cutout/worker_event_downloading.json"),
        include_str!("../../../contracts/video_cutout/worker_event_tracking.json"),
        include_str!("../../../contracts/video_cutout/worker_event_finishing.json"),
        include_str!("../../../contracts/video_cutout/worker_event_done.json"),
        include_str!("../../../contracts/video_cutout/worker_event_failed.json"),
    ];

    #[test]
    fn deserializes_all_contract_files() {
        let events: Vec<WorkerEvent> = CONTRACTS
            .iter()
            .map(|s| serde_json::from_str(s).unwrap())
            .collect();
        for e in &events {
            assert_eq!(e.job_id, "6f1c2b9e-0d4a-4c1e-9a57-3f2e8b1d7c60");
        }
        let phases: Vec<WorkerEventPhase> = events.into_iter().map(|e| e.phase).collect();
        assert_eq!(
            phases,
            vec![
                WorkerEventPhase::Downloading,
                WorkerEventPhase::Tracking { progress: 0.45 },
                WorkerEventPhase::Finishing,
                WorkerEventPhase::Done {
                    frames: 240,
                    lost_frames: 3,
                    duration_ms: 8000
                },
                WorkerEventPhase::Failed {
                    code: "unsupported_video".to_string()
                },
            ]
        );
    }

    #[test]
    fn maps_to_job_phase() {
        let failed = |c: &str| {
            WorkerEventPhase::Failed {
                code: c.to_string(),
            }
            .to_job_phase()
        };
        assert_eq!(
            failed("subject_not_found"),
            JobPhase::Failed(FailureCode::SubjectNotFound)
        );
        assert_eq!(
            failed("timeout"),
            JobPhase::Failed(FailureCode::InternalError)
        );
        assert_eq!(
            failed("whatever"),
            JobPhase::Failed(FailureCode::InternalError)
        );
        let done = WorkerEventPhase::Done {
            frames: 1,
            lost_frames: 0,
            duration_ms: 2,
        };
        assert_eq!(done.to_job_phase(), JobPhase::Done);
        assert_eq!(
            done.result(),
            Some(JobResult {
                frames: 1,
                lost_frames: 0,
                duration_ms: 2
            })
        );
        assert_eq!(WorkerEventPhase::Finishing.result(), None);
    }

    #[test]
    fn rejects_unknown_phase_and_missing_fields() {
        assert!(serde_json::from_str::<WorkerEvent>(r#"{"job_id":"x","phase":"queued"}"#).is_err());
        assert!(
            serde_json::from_str::<WorkerEvent>(r#"{"job_id":"x","phase":"tracking"}"#).is_err()
        );
        assert!(serde_json::from_str::<WorkerEvent>(r#"{"phase":"finishing"}"#).is_err());
    }
}
