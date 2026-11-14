//! `POST /video-jobs/{id}/start` のレスポンス（202）: `{ "phase": "queued" | "failed", "failure_code", "remaining_today" }`。

#[derive(serde::Serialize)]
pub struct StartVideoJobResponse {
    pub phase: &'static str,
    /// `failed` のときだけ失敗コード（`too_large` / `dispatch_failed`）。それ以外は null。
    pub failure_code: Option<&'static str>,
    pub remaining_today: u8,
}
