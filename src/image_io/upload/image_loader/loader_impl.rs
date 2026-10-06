use std::time::Instant;

use crate::application::interface::image_loader::error::LoadingError;
use crate::application::interface::image_loader::loader::ImageLoader;
use crate::application::types::loaded_image::LoadedImage;
use crate::domain::entity::image::Image;
use crate::domain::value_object::image_data::ImageData;
use crate::domain::value_object::image_id::image_id::ImageId;
use crate::domain::value_object::image_size::image_size::ImageSize;
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

        let image_size = ImageSize::new(height as u16, width as u16)?;

        let rgba_image = img.to_rgba8();
        let raw_pixels = rgba_image.into_raw();

        let image_data = ImageData::new(raw_pixels);
        let image_id = ImageId::new();
        let image = Image::new(image_data, image_id, image_size);

        let end = start.elapsed();
        println!("Loading: {:?}", end);

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
    use crate::application::interface::image_loader::{error::LoadingError, loader::ImageLoader};

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
