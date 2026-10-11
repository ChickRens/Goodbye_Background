use crate::{image::image::Image, preview::preview::PreviewImage};

pub trait PreviewImageGenerator {
    fn generate(&self, image: &Image) -> (PreviewImage, f64);
}
