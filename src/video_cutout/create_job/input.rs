//! ジョブ作成の入力。

use std::net::IpAddr;

use uuid::Uuid;

pub struct CreateVideoJobInput {
    pub client_id: Uuid,
    pub ip: IpAddr,
    /// 利用者が申告した入力動画のサイズ（バイト）。
    pub size_bytes: u64,
    pub content_type: String,
    pub start_ms: u32,
    pub end_ms: u32,
}
