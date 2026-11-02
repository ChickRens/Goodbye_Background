//! 元画像。マスク適用（切り抜き）は常にこの解像度で行う。

use crate::image::image::{id::ImageId, image::Image};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OriginalImage {
    image: Image,
}

impl OriginalImage {
    pub fn new(image: Image) -> Self {
        Self { image }
    }

    pub fn into_image(self) -> Image {
        self.image
    }

    pub fn image(&self) -> &Image {
        &self.image
    }

    pub fn image_id(&self) -> ImageId {
        *self.image.image_id()
    }
}
