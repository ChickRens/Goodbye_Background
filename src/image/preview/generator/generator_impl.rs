use std::time::Instant;

use fast_image_resize as fr;
use image::RgbaImage;
use webp::{Encoder, WebPConfig};

use crate::{image::{image::Image, raw_data::ImageData, size::ImageSize}, preview::{generator::generator::PreviewImageGenerator, preview::PreviewImage}};


#[derive(Debug)]
pub struct WebPPreviewImageGenerator {
    long_side: f64,
}

impl PreviewImageGenerator for WebPPreviewImageGenerator {
    fn generate(&self, image: &Image) -> (PreviewImage, f64) {
        let source_size = image.image_size();
        let scale = self.long_side / source_size.height().max(source_size.width()) as f64;
        let width = ((source_size.width() as f64 * scale).round() as u16).max(2);
        let height = ((source_size.height() as f64 * scale).round() as u16).max(2);
        let preview_size = ImageSize::new(height, width).expect("preview size is invalid");

        let rgba_image = RgbaImage::from_raw(
            source_size.width() as u32,
            source_size.height() as u32,
            image.image_data().image().clone(),
        )
        .expect("image data is not valid RGBA data");

        let start = Instant::now();

        let mut dst_image =
            fr::images::Image::new(width as u32, height as u32, fr::PixelType::U8x4);
        let options = fr::ResizeOptions::new().resize_alg(fr::ResizeAlg::Nearest);

        let mut resizer = fr::Resizer::new();
        resizer
            .resize(&rgba_image, &mut dst_image, &options)
            .expect("Resize Failed in PreviewGenerator");

        let end = start.elapsed();
        println!("Resize in PreviewGenerator: {:?}", end);

        let start = Instant::now();
        let dst_vec = dst_image.into_vec();
        let encoder = Encoder::from_rgba(&dst_vec, width as u32, height as u32);

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
        let end = start.elapsed();
        println!("Encode in PreviewGenerator: {:?}", end);

        (
            PreviewImage::new(Image::new(
                ImageData::new(webp),
                *image.image_id(),
                preview_size,
            )),
            scale,
        )
    }
}

#[cfg(test)]
mod preview_image_generator_test {
    use crate::{image::{id::ImageId, image::Image, raw_data::ImageData, size::ImageSize}, preview::generator::generator_impl::WebPPreviewImageGenerator};

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
}

impl WebPPreviewImageGenerator {
    pub fn new(preview_image_long_side: u16) -> Self {
        Self {
            long_side: preview_image_long_side as f64,
        }
    }
}
