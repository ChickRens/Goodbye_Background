//! 保存用画像を生成するトレイト。
//!
//! 注意: ファイル名と中身が他モジュールと逆（このファイルがトレイト側）。

use crate::image::{completed::completed_image::CompletedImage, image::image::Image};

pub trait CompletedImageGenerator: Send + Sync {
    fn generate(&self, image: &Image) -> CompletedImage;
}
