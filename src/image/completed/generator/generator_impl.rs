use crate::image::{completed::completed_image::CompletedImage, image::image::Image};

pub trait CompletedImageGenerator: Send + Sync {
    fn generate(&self, image: &Image) -> CompletedImage;
}
