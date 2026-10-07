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
