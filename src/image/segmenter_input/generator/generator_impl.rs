//! `SegmenterInputImageGenerator` の SAM2 実装。

use fast_image_resize as fr;

use crate::image::{
    image::{image::Image, raw_data::ImageData, size::ImageSize},
    original::original_image::OriginalImage,
    segmenter_input::{
        generator::generator::SegmenterInputImageGenerator,
        segmenter_input_image::SegmenterInputImage,
    },
};

/// 元画像を SAM2 入力サイズ（1024x1024）にリサイズする。
///
/// **アスペクト比を保たずに引き伸ばす**（パディングしない）。SAM2 公式の前処理と同じ方式で、
/// ポイント座標も縦横別々の倍率で変換している（`Sam2Segmenter::_scale_prompt`）ので整合している。
/// フィルタは Bilinear 畳み込み（`fast_image_resize`）。
#[derive(Debug, Clone)]
pub struct SAM2InputGenerator {
    sam2_required_height: f32,
    sam2_required_width: f32,
}

impl SegmenterInputImageGenerator for SAM2InputGenerator {
    fn generate(&self, image: &OriginalImage) -> SegmenterInputImage {
        let input_image = image.image();
        let size = input_image.image_size();

        // 元データは借用するだけ（コピーしない）。fast_image_resize は SIMD + rayon で並列に動く。
        // （以前の image::imageops::resize はシングルスレッドで、12MP 画像で約 300ms かかっていた）
        let source = fr::images::ImageRef::new(
            size.width() as u32,
            size.height() as u32,
            input_image.image_data().image(),
            fr::PixelType::U8x4,
        )
        .expect("segmenter input image data is not valid RGBA data");
        let mut resized = fr::images::Image::new(
            self.sam2_required_width as u32,
            self.sam2_required_height as u32,
            fr::PixelType::U8x4,
        );
        // Bilinear 畳み込み（縮小時はサポート幅を広げて平均する）= 旧実装の Triangle フィルタと同等。
        let options = fr::ResizeOptions::new()
            .resize_alg(fr::ResizeAlg::Convolution(fr::FilterType::Bilinear));
        fr::Resizer::new()
            .resize(&source, &mut resized, &options)
            .expect("Resize Failed in SAM2InputGenerator");

        let resized_image = Image::new(
            ImageData::new(resized.into_vec()),
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
    use super::SAM2InputGenerator;
    use crate::image::{
        image::{id::ImageId, image::Image, raw_data::ImageData, size::ImageSize},
        original::original_image::OriginalImage,
        segmenter_input::generator::generator::SegmenterInputImageGenerator,
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
