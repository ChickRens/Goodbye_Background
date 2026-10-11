use crate::image::{
    original::original_image::OriginalImage,
    segmenter_input::segmenter_input_image::SegmenterInputImage,
};

pub trait SegmenterInputImageGenerator {
    fn generate(&self, image: &OriginalImage) -> SegmenterInputImage;
}
