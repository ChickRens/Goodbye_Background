//! undo の出力。新しく発行されたプレビューの image_id。

use crate::image_cutout::image::image::id::ImageId;

pub struct UndoOutput {
    image_id: ImageId,
}

impl UndoOutput {
    pub fn new(image_id: ImageId) -> Self {
        Self { image_id }
    }

    pub fn image_id(self) -> ImageId {
        self.image_id
    }
}
