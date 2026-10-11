use crate::image::image::{id::ImageId, image::Image};

#[derive(Debug, Clone)]
pub struct CompletedImage {
    image: Image,
}

impl CompletedImage {
    pub fn new(image: Image) -> Self {
        Self { image }
    }

    pub fn into_image(self) -> Image {
        self.image
    }

    pub fn image_id(&self) -> ImageId {
        *self.image.image_id()
    }
}
