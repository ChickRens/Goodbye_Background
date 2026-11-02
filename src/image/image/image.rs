//! 画像の共通表現。中身のバイト列が「生 RGBA」か「エンコード済み WebP」かは
//! 型からは分からないので、どの文脈の `Image` かに注意すること（`image/mod.rs` の表を参照）。

use crate::image::image::{id::ImageId, raw_data::ImageData, size::ImageSize};

#[derive(Debug, PartialEq, Eq, Clone)]
pub struct Image {
    image_data: ImageData,
    image_id: ImageId,
    image_size: ImageSize,
}

impl Image {
    pub fn new(image_data: ImageData, id: ImageId, size: ImageSize) -> Self {
        Self {
            image_data: image_data,
            image_id: id,
            image_size: size,
        }
    }

    pub fn image_data(&self) -> &ImageData {
        &self.image_data
    }

    pub fn image_id(&self) -> &ImageId {
        &self.image_id
    }

    pub fn image_size(&self) -> &ImageSize {
        &self.image_size
    }

    pub fn into_data(self) -> (ImageData, ImageId, ImageSize) {
        (self.image_data, self.image_id, self.image_size)
    }
}
