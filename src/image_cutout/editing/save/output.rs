//! save の出力。`/get-completed` に渡す image_id。

use crate::image_cutout::image::image::id::ImageId;

pub struct SaveOutput {
    image_id: ImageId,
}

impl SaveOutput {
    pub fn new(image_id: ImageId) -> Self {
        Self { image_id }
    }

    pub fn image_id(&self) -> ImageId {
        self.image_id
    }
}
