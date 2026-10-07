use crate::domain::value_object::image_id::image_id::ImageId;

pub struct SaveOutput {
    image_id: ImageId,
}

impl SaveOutput {
    pub fn new(image_id: ImageId) -> Self {
        Self { image_id }
    }

    pub fn image_id(&self) -> ImageId {
        self.image_id
    }
}
