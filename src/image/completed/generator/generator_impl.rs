use crate::{application::types::completed_image::CompletedImage, domain::entity::image::Image};

pub trait CompletedImageGenerator {
    fn generate(&self, image: &Image) -> CompletedImage;
}
