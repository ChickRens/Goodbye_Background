//! ジョブ状態の取得の入力。

use std::net::IpAddr;

use uuid::Uuid;

use crate::video_cutout::job::id::VideoJobId;

pub struct GetVideoJobInput {
    pub client_id: Uuid,
    /// リクエストの IP。残り回数はジョブの持ち主のキー（作成時の IP）で数えるので、現状は使わない。
    #[allow(dead_code)]
    pub ip: IpAddr,
    pub job_id: VideoJobId,
}
