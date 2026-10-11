use crate::image::image::{id::ImageId, image::Image, raw_data::ImageData, size::ImageSize};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreviewImage {
    image: Image,
}

impl PreviewImage {
    pub fn new(image: Image) -> Self {
        Self { image }
    }

    pub fn image_id(&self) -> ImageId {
        *self.image.image_id()
    }

    pub fn into_data(self) -> (ImageData, ImageId, ImageSize) {
        self.image.into_data()
    }
}
