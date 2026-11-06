//! `/upload` のレスポンス: `{ "session_id": UUID, "image_id": UUID }`。
//! `image_id` は元画像プレビューの ID（そのまま `/get-preview` に渡せる）。

use uuid::Uuid;

#[derive(serde::Serialize)]
pub struct UploadResponse {
    session_id: Uuid,
    image_id: Uuid,
}

impl UploadResponse {
    pub fn new(session_id: Uuid, image_id: Uuid) -> Self {
        Self {
            session_id,
            image_id,
        }
    }
}
