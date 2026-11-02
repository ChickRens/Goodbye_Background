//! プレビュー生成トレイト。戻り値の `f64` は「元画像座標 → プレビュー座標」の倍率
//! （preview = original * scale）。

use crate::image::{image::image::Image, preview::preview::PreviewImage};

pub trait PreviewImageGenerator: Send + Sync {
    fn generate(&self, image: &Image) -> (PreviewImage, f64);
}
