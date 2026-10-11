use ndarray::{ArrayView4, s};
use rayon::prelude::*;

use crate::domain::entity::image::Image;

pub struct SAM2MaskApplier;

impl SAM2MaskApplier {
    pub fn apply(original: &Image, mask: ArrayView4<f32>) -> Vec<u8> {
        let mask_2d = mask.slice(s![0, 0, .., ..]);
        let original_data = original.image_data().image();

        let mut output = original_data.to_vec();

        debug_assert!(mask_2d.as_slice().is_some());

        let mask_slice = mask_2d
            .as_slice()
            .expect("Applier requires contiguous ArrayView4");

        output
            .par_chunks_exact_mut(4)
            .zip(mask_slice.par_iter())
            .for_each(|(pixel, pixel_alpha)| {
                pixel[3] = (pixel_alpha * 255.0).clamp(0.0, 255.0) as u8;
            });

        output
    }
}

#[cfg(test)]
mod mask_applier_test {
    use image::RgbaImage;
    use ndarray::Array4;

    use crate::{
        domain::{
            entity::image::Image,
            value_object::{
                image_data::ImageData, image_id::image_id::ImageId,
                image_size::image_size::ImageSize,
            },
        },
        infrastructure::segmenter::mask_applier::SAM2MaskApplier,
    };

    fn _create_5x5_rgba_image() -> Image {
        let image_pixels = [
            0, 0, 0, 255, 0, 0, 0, 255, 0, 0, 0, 255, 0, 0, 0, 255, 0, 0, 0, 255, 0, 0, 0, 255,
            255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 0, 0, 0, 255, 0, 0, 0, 255,
            255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 0, 0, 0, 255, 0, 0, 0, 255,
            255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 0, 0, 0, 255, 0, 0, 0, 255,
            0, 0, 0, 255, 0, 0, 0, 255, 0, 0, 0, 255, 0, 0, 0, 255,
        ];
        Image::new(
            ImageData::new(image_pixels.to_vec()),
            ImageId::new(),
            ImageSize::new(5, 5).unwrap(),
        )
    }

    fn _create_x_shape_5x5_mask() -> Array4<f32> {
        use ndarray::Array4;
        let mask_pixels: [f32; 25] = [
            1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 1.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0,
            0.0, 1.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0,
        ];

        Array4::from_shape_vec((1, 1, 5, 5), mask_pixels.to_vec()).unwrap()
    }

    #[ignore]
    #[test]
    fn test_visualize_apply_result() {
        let original = _create_5x5_rgba_image();
        let mask = _create_x_shape_5x5_mask();

        let applied = SAM2MaskApplier::apply(&original, mask.view());

        let (_data, _id, size) = original.into_data();

        let img = RgbaImage::from_raw(size.width() as u32, size.height() as u32, applied).unwrap();

        img.save("x_shape.png").unwrap();
    }

    #[test]
    fn test_normal_apply() {
        let original = _create_5x5_rgba_image();
        let mask = _create_x_shape_5x5_mask();

        let applied = SAM2MaskApplier::apply(&original, mask.view());

        assert_eq!(100, applied.len());
        assert_eq!(
            applied,
            [
                0, 0, 0, 255, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 255, 0, 0, 0, 0, 255,
                255, 255, 255, 255, 255, 255, 0, 255, 255, 255, 255, 0, 0, 0, 0, 0, 0, 0, 0, 255,
                255, 255, 0, 255, 255, 255, 255, 255, 255, 255, 0, 0, 0, 0, 0, 0, 0, 0, 0, 255,
                255, 255, 255, 255, 255, 255, 0, 255, 255, 255, 255, 0, 0, 0, 0, 0, 0, 0, 255, 0,
                0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 255
            ]
        );
    }
}
