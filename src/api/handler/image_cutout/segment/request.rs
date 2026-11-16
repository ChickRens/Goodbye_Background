//! `/segment` のリクエスト。
//!
//! ```json
//! { "session_id": "UUID", "x": 120, "y": 340, "is_foreground": true }
//! ```
//! `is_foreground`: true = 残したい所（前景）, false = 消したい所（背景）。
//! `x`/`y` は u16 なので負数や小数を送ると 422（axum の JSON パースエラー）になる。

#[derive(serde::Deserialize)]
pub struct SegmentRequest {
    pub session_id: String,
    pub x: u16,
    pub y: u16,
    pub is_foreground: bool,
}
