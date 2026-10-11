use crate::image::{completed::completed_image::CompletedImage, image::image::Image};

pub trait CompletedImageGenerator {
    fn generate(&self, image: &Image) -> CompletedImage;
}
