use crate::domain::value_object::{image_id::image_id::ImageId, session_id::session_id::SessionId};

#[derive(Debug, PartialEq)]
pub struct UploadOutput {
    session_id: SessionId,
    image_id: ImageId,
    preview_to_original_scale: f64,
}

impl UploadOutput {
    pub fn new(session_id: SessionId, image_id: ImageId, preview_to_original_scale: f64) -> Self {
        Self {
            session_id,
            image_id,
            preview_to_original_scale,
        }
    }

    pub fn session_id_and_image_id(&self) -> (SessionId, ImageId) {
        (self.session_id, self.image_id)
    }

    pub fn scale(&self) -> f64 {
        self.preview_to_original_scale
    }
}
