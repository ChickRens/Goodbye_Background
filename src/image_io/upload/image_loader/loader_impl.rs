//! `ImageLoader` の実装（`image` クレート）。
//!
//! - 形式はバイト列の中身から推測（拡張子・Content-Type は見ない）。対応形式は image クレートの
//!   有効な feature 次第（デフォルト feature: PNG/JPEG/WebP/GIF/BMP/TIFF 等。HEIC は非対応）
//! - EXIF の Orientation を反映してから RGBA8 に変換する（スマホ写真が横倒しにならないように）
//! - 16bit 画像や透過なし画像も全て RGBA8 に正規化される

use std::time::Instant;

use crate::image::image::{id::ImageId, image::Image, raw_data::ImageData, size::ImageSize};
use crate::image_io::upload::image_loader::{
    error::LoadingError, loaded_image::LoadedImage, loader::ImageLoader,
};
use image::{DynamicImage, ImageDecoder, ImageError, ImageReader};
use std::io::Cursor;

pub struct FileImageLoader;

impl ImageLoader for FileImageLoader {
    fn load(&self, data: Vec<u8>) -> Result<LoadedImage, LoadingError> {
        let start = Instant::now();

        let reader = ImageReader::new(Cursor::new(&data))
            .with_guessed_format()
            .map_err(|err| LoadingError::CorruptedImage(err.to_string()))?;

        let mut decoder = reader.into_decoder().map_err(|err| match err {
            ImageError::Unsupported(e) => LoadingError::UnsupportedFormat(e.to_string()),
            ImageError::Decoding(e) => LoadingError::CorruptedImage(e.to_string()),
            error => LoadingError::CorruptedImage(error.to_string()),
        })?;

        // EXIF Orientationを取得
        let orientation = decoder
            .orientation()
            .map_err(|err| LoadingError::CorruptedImage(err.to_string()))?;

        // 画像をデコード
        let mut img = DynamicImage::from_decoder(decoder).map_err(|err| match err {
            ImageError::Unsupported(e) => LoadingError::UnsupportedFormat(e.to_string()),
            ImageError::Decoding(e) => LoadingError::CorruptedImage(e.to_string()),
            error => LoadingError::CorruptedImage(error.to_string()),
        })?;

        // EXIF Orientationを実際のピクセルに反映
        img.apply_orientation(orientation);

        // Orientation適用後のサイズを取得する
        let width = img.width();
        let height = img.height();

        // u32 のまま渡す（先に `as u16` すると 65535 超が回り込んで検証をすり抜けるため）。
        let image_size = ImageSize::from_u32(height, width)?;

        let rgba_image = img.to_rgba8();
        let raw_pixels = rgba_image.into_raw();

        let image_data = ImageData::new(raw_pixels);
        let image_id = ImageId::new();
        let image = Image::new(image_data, image_id, image_size);

        let end = start.elapsed();
        tracing::debug!(elapsed = ?end, "upload: decode image");

        Ok(LoadedImage::new(image))
    }
}

impl FileImageLoader {
    pub fn new() -> Self {
        Self
    }
}

#[cfg(test)]
mod image_loader_tests {
    use std::{fs::read, path::Path};

    use super::FileImageLoader;
    use crate::{
        image::image::error::ImageSizeError,
        image_io::upload::image_loader::{error::LoadingError, loader::ImageLoader},
    };

    #[test]
    fn test_normal_load() {
        let loader = FileImageLoader::new();

        let image =
            read(Path::new(env!("CARGO_MANIFEST_DIR")).join("test/test_image/Normal_Image.png"))
                .unwrap();

        let result = loader.load(image);
        assert!(result.is_ok());

        let image = result.unwrap().into_image();
        let size = image.image_size();
        assert_eq!(size.height(), 500);
        assert_eq!(size.width(), 500);
    }

    #[test]
    fn test_broken_image() {
        let loader = FileImageLoader::new();

        let image =
            read(Path::new(env!("CARGO_MANIFEST_DIR")).join("test/test_image/Broken_Image.png"))
                .unwrap();

        let result = loader.load(image);
        assert!(matches!(result, Err(LoadingError::CorruptedImage(_))))
    }

    // u16 に収まらない幅（70000 = 65536 + 4464）でも回り込まずに LongWidth で弾くこと。
    #[test]
    fn test_width_over_u16_max_is_rejected() {
        let png = encode_png(70_000, 2);

        let result = FileImageLoader::new().load(png);

        assert!(matches!(
            result,
            Err(LoadingError::ImageSize(ImageSizeError::LongWidth))
        ));
    }

    #[test]
    fn test_height_over_u16_max_is_rejected() {
        let png = encode_png(2, 70_000);

        let result = FileImageLoader::new().load(png);

        assert!(matches!(
            result,
            Err(LoadingError::ImageSize(ImageSizeError::LongHeight))
        ));
    }

    fn encode_png(width: u32, height: u32) -> Vec<u8> {
        let image = image::RgbaImage::new(width, height);
        let mut bytes = Vec::new();
        image
            .write_to(
                &mut std::io::Cursor::new(&mut bytes),
                image::ImageFormat::Png,
            )
            .unwrap();
        bytes
    }

    #[test]
    fn test_unsupported_extension() {
        let loader = FileImageLoader::new();

        let image =
            read(Path::new(env!("CARGO_MANIFEST_DIR")).join("test/test_image/Unsupported.wav"))
                .unwrap();

        let result = loader.load(image);
        assert!(matches!(result, Err(LoadingError::UnsupportedFormat(_))));
    }
}
