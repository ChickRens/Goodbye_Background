use std::sync::Arc;

use crate::image::{
    original::original_image::OriginalImage,
    segmenter_input::{
        generator::{generator::SegmenterInputImageGenerator, generator_impl::SAM2InputGenerator},
        segmenter_input_image::SegmenterInputImage,
    },
};

#[derive(Debug)]
pub struct SharedSAM2InputGenerator {
    generator: Arc<SAM2InputGenerator>,
}

impl SegmenterInputImageGenerator for SharedSAM2InputGenerator {
    fn generate(&self, image: &OriginalImage) -> SegmenterInputImage {
        self.generator.generate(image)
    }
}

impl SharedSAM2InputGenerator {
    pub fn new(height: u16, width: u16) -> Self {
        Self {
            generator: Arc::new(SAM2InputGenerator::new(height, width)),
        }
    }
}
