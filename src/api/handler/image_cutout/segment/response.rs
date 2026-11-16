//! `/segment` のレスポンス: `{ "image_id": UUID }`（新しい切り抜き結果プレビューの ID）。

use uuid::Uuid;

#[derive(serde::Serialize)]
pub struct SegmentResponse {
    image_id: Uuid,
}

impl SegmentResponse {
    pub fn new(image_id: Uuid) -> Self {
        Self { image_id }
    }
}
