use crate::image::{image::image::Image, preview::preview::PreviewImage};

pub trait PreviewImageGenerator: Send + Sync {
    fn generate(&self, image: &Image) -> (PreviewImage, f64);
}
