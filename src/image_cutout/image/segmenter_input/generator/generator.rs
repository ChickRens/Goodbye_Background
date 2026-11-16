//! SAM2 入力画像を生成するトレイト。

use crate::image_cutout::image::{
    original::original_image::OriginalImage,
    segmenter_input::segmenter_input_image::SegmenterInputImage,
};

pub trait SegmenterInputImageGenerator: Send + Sync {
    fn generate(&self, image: &OriginalImage) -> SegmenterInputImage;
}
