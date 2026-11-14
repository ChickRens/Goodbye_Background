//! `POST /video-jobs` のレスポンス（201）: `{ "job_id", "upload_url", "remaining_today" }`。

use uuid::Uuid;

#[derive(serde::Serialize)]
pub struct CreateVideoJobResponse {
    pub job_id: Uuid,
    /// 入力動画を PUT する署名付き URL（15 分有効）。
    pub upload_url: String,
    pub remaining_today: u8,
}
