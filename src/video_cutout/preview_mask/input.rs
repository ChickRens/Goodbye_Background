//! プレビューのマスクの入力。

use uuid::Uuid;

use crate::video_cutout::job::{id::VideoJobId, point::Points};

pub struct PreviewMaskInput {
    pub client_id: Uuid,
    pub job_id: VideoJobId,
    pub points: Points,
}
