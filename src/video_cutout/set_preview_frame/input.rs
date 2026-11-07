//! プレビュー用フレームの登録の入力。

use uuid::Uuid;

use crate::video_cutout::job::id::VideoJobId;

pub struct SetPreviewFrameInput {
    pub client_id: Uuid,
    pub job_id: VideoJobId,
    /// PNG / JPEG のバイト列（まだデコードしていない）。
    pub image: Vec<u8>,
}
