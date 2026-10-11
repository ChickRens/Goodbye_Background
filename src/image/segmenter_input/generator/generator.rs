use crate::{
    application::types::segmenter_input_image::SegmenterInputImage,
    domain::entity::original_image::OriginalImage,
};

pub trait SegmenterInputImageGenerator {
    fn generate(&self, image: &OriginalImage) -> SegmenterInputImage;
}
