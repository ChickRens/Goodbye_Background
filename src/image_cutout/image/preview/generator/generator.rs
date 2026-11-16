//! プレビュー生成トレイト。戻り値の `f64` は「元画像座標 → プレビュー座標」の倍率
//! （preview = original * scale）。

use crate::image_cutout::image::{image::image::Image, preview::preview::PreviewImage};

pub trait PreviewImageGenerator: Send + Sync {
    fn generate(&self, image: &Image) -> (PreviewImage, f64);

    /// `generate` と同じプレビューサイズにリサイズした **生 RGBA**（エンコードしない）を返す。ID は元のまま。
    /// クリックごとの切り抜きをプレビュー解像度で行うために使う（フル解像度で切り抜くより桁違いに速い）。
    fn resize_to_preview(&self, image: &Image) -> Image;
}
