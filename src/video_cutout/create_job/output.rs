//! ジョブ作成の出力。

use crate::video_cutout::job::id::VideoJobId;

pub struct CreateVideoJobOutput {
    pub job_id: VideoJobId,
    /// 入力動画をアップロードする署名付き PUT URL（15 分有効）。
    pub upload_url: String,
    /// 今日の残り回数（作成では消費しない）。
    pub remaining_today: u8,
}
