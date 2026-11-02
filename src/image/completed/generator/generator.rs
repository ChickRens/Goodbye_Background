//! `CompletedImageGenerator` の実装: 生 RGBA を WebP(quality 95, 非可逆, method 2) にエンコードする。
//!
//! `method`（0〜6）は圧縮の頑張り度で、画質（quality）とは別。小さいほど速いがファイルが大きくなる。
//! 4000x3000 での計測（2026-11）:
//!
//! | method | 時間 | サイズ |
//! |---|---|---|
//! | 4（libwebp の既定。以前の設定） | 2.55s | 1.12MB |
//! | **2（採用）** | 1.86s | 1.44MB |
//! | 0 | 0.87s | 1.74MB |
//!
//! 速度とサイズの中間として 2 を採用した（オーナー判断）。
//!
//! 注意: ファイル名と中身が他モジュールと逆（このファイルが実装側）。

use webp::{Encoder, WebPConfig};

use crate::image::{
    completed::{
        completed_image::CompletedImage, generator::generator_impl::CompletedImageGenerator,
    },
    image::{image::Image, raw_data::ImageData},
};

#[derive(Debug, Clone)]
pub struct WebPCompletedImageGenerator;

impl CompletedImageGenerator for WebPCompletedImageGenerator {
    fn generate(&self, image: &Image) -> CompletedImage {
        let size = image.image_size();

        let encoder = Encoder::from_rgba(
            image.image_data().image(),
            size.width() as u32,
            size.height() as u32,
        );

        let mut config = WebPConfig::new().expect("WebPConfig init failed");
        config.quality = 95.0;
        config.method = 2;

        let webp = encoder
            .encode_advanced(&config)
            .expect("WebP encode failed")
            .to_vec();
        CompletedImage::new(Image::new(
            ImageData::new(webp),
            *image.image_id(),
            image.image_size().clone(),
        ))
    }
}

impl WebPCompletedImageGenerator {
    pub fn new() -> Self {
        Self
    }
}

#[cfg(test)]
mod completed_image_generator_test {
    use super::WebPCompletedImageGenerator;
    use crate::image::{
        completed::generator::generator_impl::CompletedImageGenerator,
        image::{id::ImageId, image::Image, raw_data::ImageData, size::ImageSize},
    };

    #[test]
    fn generate_returns_webp_bytes_and_preserves_metadata() {
        let size = ImageSize::new(2, 2).unwrap();
        let image_id = ImageId::new();
        let rgba = vec![
            255, 0, 0, 255, 0, 255, 0, 255, 0, 0, 255, 255, 255, 255, 255, 255,
        ];
        let original = Image::new(ImageData::new(rgba), image_id, size.clone());

        let generated = WebPCompletedImageGenerator::new().generate(&original);
        let encoded = generated.into_image();
        let bytes = encoded.image_data().image();

        assert_eq!(bytes.len() >= 12, true);
        assert_eq!(&bytes[..4], b"RIFF");
        assert_eq!(&bytes[8..12], b"WEBP");
        assert_eq!(encoded.image_id(), original.image_id());
        assert_eq!(encoded.image_size(), original.image_size());
    }
}
