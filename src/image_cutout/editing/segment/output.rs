//! segment の出力。新しく発行された切り抜き結果プレビューの image_id。

use crate::image_cutout::image::image::id::ImageId;

pub struct SegmentOutput {
    image_id: ImageId,
}

impl SegmentOutput {
    pub fn new(image_id: ImageId) -> Self {
        Self { image_id }
    }

    pub fn image_id(&self) -> ImageId {
        self.image_id
    }
}
