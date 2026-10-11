use image::{
    RgbaImage,
    imageops::{self, FilterType},
};

use crate::{
    application::{
        interface::segmenter_input_image_generator::SegmenterInputImageGenerator,
        types::segmenter_input_image::SegmenterInputImage,
    },
    domain::{
        entity::{image::Image, original_image::OriginalImage},
        value_object::{image_data::ImageData, image_size::image_size::ImageSize},
    },
};

#[derive(Debug)]
pub struct SAM2InputGenerator {
    sam2_required_height: f32,
    sam2_required_width: f32,
}

impl SegmenterInputImageGenerator for SAM2InputGenerator {
    fn generate(&self, image: &OriginalImage) -> SegmenterInputImage {
        let input_image = image.image();
        let size = input_image.image_size();

        let rgba_image = RgbaImage::from_raw(
            size.width() as u32,
            size.height() as u32,
            input_image.image_data().image().clone(),
        )
        .expect("segmenter input image data is not valid RGBA data");

        let resized = imageops::resize(
            &rgba_image,
            self.sam2_required_width as u32,
            self.sam2_required_height as u32,
            FilterType::Triangle,
        );

        let resized_image = Image::new(
            ImageData::new(resized.into_raw()),
            *input_image.image_id(),
            ImageSize::new(
                self.sam2_required_height as u16,
                self.sam2_required_width as u16,
            )
            .expect("segmenter input image size is invalid"),
        );

        SegmenterInputImage::new(resized_image)
    }
}

#[cfg(test)]
mod segmenter_input_image_generator_test {
    use crate::{
        application::interface::segmenter_input_image_generator::SegmenterInputImageGenerator,
        domain::{
            entity::{image::Image, original_image::OriginalImage},
            value_object::{
                image_data::ImageData, image_id::image_id::ImageId,
                image_size::image_size::ImageSize,
            },
        },
        infrastructure::generator::segmenter_input::SAM2InputGenerator,
    };

    #[test]
    fn generate_returns_sam2_input() {
        let size = ImageSize::new(2, 2).unwrap();
        let image_id = ImageId::new();
        let rgba = vec![
            255, 0, 0, 255, 0, 255, 0, 255, 0, 0, 255, 255, 255, 255, 255, 255,
        ];
        let original = OriginalImage::new(Image::new(ImageData::new(rgba), image_id, size.clone()));

        let input = SAM2InputGenerator::new(4, 4).generate(&original);
        let (_, input_image_id, input_size) = input.image().clone().into_data();

        assert_eq!(input_image_id, *original.image().image_id());
        assert_eq!(input_size, ImageSize::new(4, 4).unwrap());
    }
}

impl SAM2InputGenerator {
    pub fn new(height: u16, width: u16) -> Self {
        Self {
            sam2_required_height: height as f32,
            sam2_required_width: width as f32,
        }
    }
}
