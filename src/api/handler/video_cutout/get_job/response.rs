//! `GET /video-jobs/{id}` のレスポンス（200）。
//!
//! ```json
//! { "phase": "tracking", "progress": 0.25, "failure_code": null, "quota_refunded": false,
//!   "download_url": null, "remaining_today": 2 }
//! ```
//! `phase` は `created` / `queued` / `downloading` / `tracking` / `finishing` / `done` / `failed`。
//! `progress` は `tracking` のときだけ 0.0〜1.0、`download_url` は `done` のときだけ（10 分有効）。

#[derive(serde::Serialize)]
pub struct GetVideoJobResponse {
    pub phase: &'static str,
    pub progress: Option<f32>,
    pub failure_code: Option<&'static str>,
    pub quota_refunded: bool,
    pub download_url: Option<String>,
    pub remaining_today: u8,
}
