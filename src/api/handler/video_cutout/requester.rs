//! 動画 API の呼び出し元（`client-id` ヘッダーと IP）の取り出し。

use std::net::{IpAddr, SocketAddr};

use axum::http::HeaderMap;
use uuid::Uuid;

use crate::{
    api::handler::video_cutout::error::VideoRequestError,
    composition::video_wiring::VideoCutoutApp, presentation::client_ip::client_ip,
};

pub struct Requester {
    pub client_id: Uuid,
    pub ip: IpAddr,
}

impl Requester {
    /// `client-id` ヘッダーが無い・UUID でなければ `MISSING_CLIENT_ID`。
    pub fn from_request(
        headers: &HeaderMap,
        peer: SocketAddr,
        video: &VideoCutoutApp,
    ) -> Result<Self, VideoRequestError> {
        let client_id = headers
            .get("client-id")
            .and_then(|v| v.to_str().ok())
            .and_then(|v| Uuid::parse_str(v.trim()).ok())
            .ok_or(VideoRequestError::MissingClientId)?;
        Ok(Self {
            client_id,
            ip: client_ip(headers, peer, video.trusted_proxy_hops()),
        })
    }
}
