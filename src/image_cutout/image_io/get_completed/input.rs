//! 完成画像取得の入力。image_id は `/save` のレスポンスで返したもの。

use crate::image_cutout::image::image::id::ImageId;

pub struct GetCompletedInput {
    image_id: ImageId,
}

impl GetCompletedInput {
    pub fn new(image_id: ImageId) -> Self {
        Self { image_id }
    }

    pub fn image_id(&self) -> ImageId {
        self.image_id
    }
}
