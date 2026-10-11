use std::sync::Arc;

use webp::Encoder;

use crate::{
    application::{
        interface::completed_image_generator::CompletedImageGenerator,
        types::completed_image::CompletedImage,
    },
    domain::{entity::image::Image, value_object::image_data::ImageData},
};

#[derive(Debug)]
pub struct WebPCompletedImageGenerator;

impl CompletedImageGenerator for WebPCompletedImageGenerator {
    fn generate(&self, image: &Image) -> CompletedImage {
        let size = image.image_size();

        let encoder = Encoder::from_rgba(
            image.image_data().image(),
            size.width() as u32,
            size.height() as u32,
        );

        let webp = encoder.encode(95.0).to_vec();
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
    use crate::{
        application::interface::completed_image_generator::CompletedImageGenerator,
        domain::{
            entity::image::Image,
            value_object::{
                image_data::ImageData, image_id::image_id::ImageId,
                image_size::image_size::ImageSize,
            },
        },
        infrastructure::generator::completed_image::WebPCompletedImageGenerator,
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

#[derive(Debug, Clone)]
pub struct SharedWebPCompletedImageGenerator {
    generator: Arc<WebPCompletedImageGenerator>,
}

impl CompletedImageGenerator for SharedWebPCompletedImageGenerator {
    fn generate(&self, image: &Image) -> CompletedImage {
        self.generator.generate(image)
    }
}

impl SharedWebPCompletedImageGenerator {
    pub fn new() -> Self {
        Self {
            generator: Arc::new(WebPCompletedImageGenerator::new()),
        }
    }
}
