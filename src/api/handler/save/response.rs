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
