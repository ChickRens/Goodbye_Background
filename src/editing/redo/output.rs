use crate::domain::value_object::image_id::image_id::ImageId;

pub struct RedoOutput {
    image_id: ImageId,
}

impl RedoOutput {
    pub fn new(image_id: ImageId) -> Self {
        Self { image_id }
    }

    pub fn image_id(self) -> ImageId {
        self.image_id
    }
}
