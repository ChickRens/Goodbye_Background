use std::sync::Arc;

use crate::{
    application::{
        interface::segmenter_input_image_generator::SegmenterInputImageGenerator,
        types::segmenter_input_image::SegmenterInputImage,
    },
    domain::entity::original_image::OriginalImage,
    infrastructure::generator::segmenter_input::SAM2InputGenerator,
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
