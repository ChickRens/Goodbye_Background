//! `/save` のレスポンス: `{ "image_id": UUID }`。`/get-completed` に渡す ID
//! （`/get-preview` では取得できない）。

use uuid::Uuid;

#[derive(serde::Serialize)]
pub struct SaveResponse {
    image_id: Uuid,
}

impl SaveResponse {
    pub fn new(image_id: Uuid) -> Self {
        Self { image_id }
    }
}
