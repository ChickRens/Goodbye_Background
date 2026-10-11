use crate::domain::{entity::image::Image, value_object::image_id::image_id::ImageId};

#[derive(Debug, PartialEq, Eq, Clone)]
pub struct SegmenterInputImage {
    image: Image,
}

impl SegmenterInputImage {
    pub fn new(image: Image) -> Self {
        Self { image }
    }

    pub fn image(&self) -> &Image {
        &self.image
    }

    pub fn image_id(&self) -> &ImageId {
        self.image.image_id()
    }
}
