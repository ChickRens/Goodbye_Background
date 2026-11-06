//! ジョブ開始の出力。

use crate::video_cutout::job::phase::JobPhase;

pub struct StartVideoJobOutput {
    /// 開始後の状態。`Queued`、または `Failed(TooLarge)` / `Failed(DispatchFailed)`。
    pub phase: JobPhase,
    /// 今日の残り回数（消費・返却を反映した後の値）。
    pub remaining_today: u8,
}
