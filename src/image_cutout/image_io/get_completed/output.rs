//! 完成画像取得の出力（WebP バイト列 + サイズ）。

use crate::image_cutout::image::image::size::ImageSize;

pub struct GetCompletedOutput {
    data: Vec<u8>,
    size: ImageSize,
}

impl GetCompletedOutput {
    pub fn new(data: Vec<u8>, size: ImageSize) -> Self {
        Self { data: data, size }
    }

    pub fn image_data(self) -> (Vec<u8>, ImageSize) {
        (self.data, self.size)
    }
}
