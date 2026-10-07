use crate::domain::value_object::{image_id::image_id::ImageId, session_id::session_id::SessionId};

pub struct PrepareSegmentInput {
    image_id: ImageId,
    session_id: SessionId,
    point_scale: f64,
}

impl PrepareSegmentInput {
    pub fn new(
        image_id: ImageId,
        session_id: SessionId,
        preview_to_original_point_scale: f64,
    ) -> Self {
        Self {
            image_id,
            session_id,
            point_scale: preview_to_original_point_scale,
        }
    }

    pub fn image_id(&self) -> ImageId {
        self.image_id
    }

    pub fn session_id(&self) -> SessionId {
        self.session_id
    }

    pub fn point_scale(&self) -> f64 {
        self.point_scale
    }
}
