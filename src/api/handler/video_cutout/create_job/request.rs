//! `POST /video-jobs` のリクエスト。
//!
//! ```json
//! { "size_bytes": 10000000, "content_type": "video/mp4", "start_ms": 1000, "end_ms": 9000 }
//! ```

#[derive(serde::Deserialize)]
pub struct CreateVideoJobRequest {
    pub size_bytes: u64,
    pub content_type: String,
    pub start_ms: u32,
    pub end_ms: u32,
}
