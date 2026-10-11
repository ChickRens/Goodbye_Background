use std::sync::Arc;

use crate::image::{
    image::image::Image,
    preview::{
        generator::{generator::PreviewImageGenerator, generator_impl::WebPPreviewImageGenerator},
        preview::PreviewImage,
    },
};

#[derive(Debug, Clone)]
pub struct SharedPreviewGenerator {
    generator: Arc<WebPPreviewImageGenerator>,
}

impl PreviewImageGenerator for SharedPreviewGenerator {
    fn generate(&self, image: &Image) -> (PreviewImage, f64) {
        self.generator.generate(image)
    }
}

impl SharedPreviewGenerator {
    pub fn new(preview_image_long_side: u16) -> Self {
        Self {
            generator: Arc::new(WebPPreviewImageGenerator::new(preview_image_long_side)),
        }
    }
}
