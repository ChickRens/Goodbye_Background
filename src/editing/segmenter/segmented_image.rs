use crate::image::image::{raw_data::ImageData, size::ImageSize};

pub struct SegmentedImage {
    image: ImageData,
    size: ImageSize,
}

impl SegmentedImage {
    pub fn new(image: ImageData, image_size: ImageSize) -> Self {
        Self {
            image,
            size: image_size,
        }
    }

    pub fn into_image_and_size(self) -> (ImageData, ImageSize) {
        (self.image, self.size)
    }
}
