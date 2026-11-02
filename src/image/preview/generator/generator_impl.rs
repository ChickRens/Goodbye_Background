//! `PreviewImageGenerator` の実装: 長辺 `long_side`px（本番は 600）に縮小して WebP にエンコードする。
//!
//! 速度優先の設定（プレビューは segment のたびに作られるため）:
//! - リサイズは Nearest（最近傍）。ジャギーが出るが速い
//! - WebP は quality 75, method 0（最速）, アルファ品質 50
//!
//! 元画像が 600px より小さい場合は **拡大** される（scale > 1）。
//!
//! `resize_to_preview` はエンコードせずに同じサイズの生 RGBA を返す。segment/undo/redo は
//! これで縮小した元画像を切り抜くので、フル解像度で処理するより桁違いに速い。

use std::time::Instant;

use fast_image_resize as fr;
use webp::{Encoder, WebPConfig};

use crate::image::{
    image::{image::Image, raw_data::ImageData, size::ImageSize},
    preview::{generator::generator::PreviewImageGenerator, preview::PreviewImage},
};

#[derive(Debug, Clone)]
pub struct WebPPreviewImageGenerator {
    long_side: f64,
}

impl PreviewImageGenerator for WebPPreviewImageGenerator {
    fn generate(&self, image: &Image) -> (PreviewImage, f64) {
        let start = Instant::now();
        let (preview_size, scale) = self.preview_size_and_scale(image.image_size());
        let resized = self.resize_rgba(image, &preview_size);
        tracing::debug!(elapsed = ?start.elapsed(), "preview: resize");

        let start = Instant::now();
        let encoder = Encoder::from_rgba(
            &resized,
            preview_size.width() as u32,
            preview_size.height() as u32,
        );

        let mut config = WebPConfig::new().unwrap();
        config.quality = 75.0;
        config.method = 0;
        config.thread_level = 1;
        config.alpha_quality = 50;
        config.alpha_filtering = 0;
        config.alpha_compression = 1;
        config.filter_strength = 0;
        config.segments = 1;
        config.sns_strength = 0;

        let webp = encoder
            .encode_advanced(&config)
            .expect("Encode Failed")
            .to_vec();
        tracing::debug!(elapsed = ?start.elapsed(), "preview: encode");

        (
            PreviewImage::new(Image::new(
                ImageData::new(webp),
                *image.image_id(),
                preview_size,
            )),
            scale,
        )
    }

    fn resize_to_preview(&self, image: &Image) -> Image {
        let (preview_size, _) = self.preview_size_and_scale(image.image_size());
        let resized = self.resize_rgba(image, &preview_size);
        Image::new(ImageData::new(resized), *image.image_id(), preview_size)
    }
}

impl WebPPreviewImageGenerator {
    /// プレビューのサイズと倍率を求める。scale = プレビュー / 元画像。
    /// フロントのクリック座標 ÷ scale = 元画像座標。
    fn preview_size_and_scale(&self, source_size: &ImageSize) -> (ImageSize, f64) {
        let scale = self.long_side / source_size.height().max(source_size.width()) as f64;
        let width = ((source_size.width() as f64 * scale).round() as u16).max(2);
        let height = ((source_size.height() as f64 * scale).round() as u16).max(2);
        let preview_size = ImageSize::new(height, width).expect("preview size is invalid");
        (preview_size, scale)
    }

    /// 生 RGBA を Nearest でリサイズする。元データは借用するだけでコピーしない
    /// （以前は 4000x3000 で約 48MB を毎回 clone していた）。同じサイズならそのまま複製して返す。
    fn resize_rgba(&self, image: &Image, target: &ImageSize) -> Vec<u8> {
        let source_size = image.image_size();
        let source_data = image.image_data().image();
        if source_size == target {
            return source_data.clone();
        }

        let source = fr::images::ImageRef::new(
            source_size.width() as u32,
            source_size.height() as u32,
            source_data,
            fr::PixelType::U8x4,
        )
        .expect("image data is not valid RGBA data");
        let mut destination = fr::images::Image::new(
            target.width() as u32,
            target.height() as u32,
            fr::PixelType::U8x4,
        );
        let options = fr::ResizeOptions::new().resize_alg(fr::ResizeAlg::Nearest);
        fr::Resizer::new()
            .resize(&source, &mut destination, &options)
            .expect("Resize Failed in PreviewGenerator");
        destination.into_vec()
    }
}

#[cfg(test)]
mod preview_image_generator_test {
    use crate::image::{
        image::{id::ImageId, image::Image, raw_data::ImageData, size::ImageSize},
        preview::generator::{
            generator::PreviewImageGenerator, generator_impl::WebPPreviewImageGenerator,
        },
    };

    #[test]
    fn generate_returns_webp_preview_and_preserves_metadata() {
        let size = ImageSize::new(2, 2).unwrap();
        let image_id = ImageId::new();
        let rgba = vec![
            255, 0, 0, 255, 0, 255, 0, 255, 0, 0, 255, 255, 255, 255, 255, 255,
        ];
        let original = Image::new(ImageData::new(rgba), image_id, size.clone());

        let (preview, scale) = WebPPreviewImageGenerator::new(4).generate(&original);
        let (preview_data, preview_image_id, preview_size) = preview.into_data();
        let bytes = preview_data.image();

        assert_eq!(scale, 2.0);
        assert_eq!(bytes.len() >= 12, true);
        assert_eq!(&bytes[..4], b"RIFF");
        assert_eq!(&bytes[8..12], b"WEBP");
        assert_eq!(preview_image_id, *original.image_id());
        assert_eq!(preview_size, ImageSize::new(4, 4).unwrap());
    }

    // エンコードせず、generate と同じプレビューサイズの生 RGBA を返すこと（クリック時の切り抜き用）。
    #[test]
    fn resize_to_preview_returns_raw_rgba_with_preview_size() {
        let size = ImageSize::new(4, 8).unwrap();
        let image_id = ImageId::new();
        let rgba = [10u8, 20, 30, 255].repeat(4 * 8);
        let original = Image::new(ImageData::new(rgba), image_id, size);

        let resized = WebPPreviewImageGenerator::new(4).resize_to_preview(&original);

        assert_eq!(resized.image_size(), &ImageSize::new(2, 4).unwrap());
        assert_eq!(resized.image_id(), &image_id);
        assert_eq!(
            resized.image_data().image(),
            &[10u8, 20, 30, 255].repeat(2 * 4)
        );
    }
}

impl WebPPreviewImageGenerator {
    pub fn new(preview_image_long_side: u16) -> Self {
        Self {
            long_side: preview_image_long_side as f64,
        }
    }
}
