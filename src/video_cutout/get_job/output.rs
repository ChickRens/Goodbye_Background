//! ジョブ状態の取得の出力。

use crate::video_cutout::job::phase::{FailureCode, JobPhase};

pub struct GetVideoJobOutput {
    pub phase: JobPhase,
    /// `Failed` のときだけ失敗コード。
    pub failure: Option<FailureCode>,
    pub quota_refunded: bool,
    /// `Done` のときだけ、出力動画の 10 分有効の GET URL。
    pub download_url: Option<String>,
    /// 今日の残り回数（ジョブの持ち主のキーで数える）。
    pub remaining_today: u8,
}
