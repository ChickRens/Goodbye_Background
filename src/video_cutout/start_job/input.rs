//! ジョブ開始の入力。

use std::net::IpAddr;

use uuid::Uuid;

use crate::video_cutout::job::{background::Background, id::VideoJobId, prompt::Prompt};

pub struct StartVideoJobInput {
    pub client_id: Uuid,
    /// 開始リクエストの IP。回数はジョブの持ち主のキー（作成時の IP）で数えるので、現状は使わない。
    #[allow(dead_code)]
    pub ip: IpAddr,
    pub job_id: VideoJobId,
    /// 点を打ったフレーム（まだ並びの規則を検査していない。ユースケースが `Prompts::new` で検査する）。
    pub prompts: Vec<Prompt>,
    pub background: Background,
}
