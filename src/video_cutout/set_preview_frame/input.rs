//! プレビュー用フレームの登録の入力。

use uuid::Uuid;

use crate::video_cutout::job::id::VideoJobId;

pub struct SetPreviewFrameInput {
    pub client_id: Uuid,
    pub job_id: VideoJobId,
    /// フレームの時刻（元の動画のミリ秒。範囲の中）。点を打つフレームを見分けるキー。
    pub time_ms: u32,
    /// PNG / JPEG のバイト列（まだデコードしていない）。
    pub image: Vec<u8>,
}
