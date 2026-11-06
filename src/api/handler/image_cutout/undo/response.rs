//! `/undo` のレスポンス: `{ "image_id": UUID }`（新しいプレビューの ID）。

use uuid::Uuid;

#[derive(serde::Serialize)]
pub struct UndoResponse {
    image_id: Uuid,
}

impl UndoResponse {
    pub fn new(image_id: Uuid) -> Self {
        Self { image_id }
    }
}
