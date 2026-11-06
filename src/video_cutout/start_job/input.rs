//! ジョブ開始の入力。

use std::net::IpAddr;

use uuid::Uuid;

use crate::video_cutout::job::{background::Background, id::VideoJobId, point::Points};

pub struct StartVideoJobInput {
    pub client_id: Uuid,
    /// 開始リクエストの IP。回数はジョブの持ち主のキー（作成時の IP）で数えるので、現状は使わない。
    #[allow(dead_code)]
    pub ip: IpAddr,
    pub job_id: VideoJobId,
    pub points: Points,
    pub background: Background,
}
