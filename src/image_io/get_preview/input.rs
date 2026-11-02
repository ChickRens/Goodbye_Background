//! プレビュー取得の入力。image_id は `/upload` `/segment` `/undo` `/redo` のレスポンスで返したもの。

use crate::image::image::id::ImageId;

pub struct GetPreviewInput {
    image_id: ImageId,
}

impl GetPreviewInput {
    pub fn new(image_id: ImageId) -> Self {
        Self { image_id }
    }

    pub fn image_id(&self) -> ImageId {
        self.image_id
    }
}
