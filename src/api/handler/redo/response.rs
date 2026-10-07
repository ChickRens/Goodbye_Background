use uuid::Uuid;

#[derive(serde::Serialize)]
pub struct RedoResponse {
    image_id: Uuid,
}

impl RedoResponse {
    pub fn new(image_id: Uuid) -> Self {
        Self { image_id }
    }
}
