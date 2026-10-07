#[derive(serde::Deserialize)]
pub struct SegmentRequest {
    pub session_id: String,
    pub x: u16,
    pub y: u16,
    pub is_foreground: bool,
}
