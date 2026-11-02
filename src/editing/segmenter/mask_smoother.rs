//! ガイデッドフィルタ（He et al. 2010）によるマスクのエッジ平滑化。`/save`（高品質版）でのみ使う。
//!
//! 目的: 256x256 から拡大したマスクはエッジがぼやける/ガタつくので、元画像の輝度エッジに
//! 沿うように補正する（髪の毛などの境界がきれいになる）。
//!
//! アルゴリズム（各ピクセルの周囲 (2r+1)^2 の窓で）:
//! - I = ガイド画像（元画像のグレースケール 0〜1）, p = 入力マスク
//! - a = cov(I, p) / (var(I) + eps), b = mean(p) - a * mean(I)
//! - 出力 = a * I + b
//! - 窓内の平均は積分画像（summed-area table）で O(1) に計算する
//!
//! パラメータ: radius = 16, eps = 0.1（ハードコード）。
//!
//! 入力は 0〜1 の確率（呼び出し側で sigmoid 済み）。
//!
//! 最後に「元のマスク値が 0.5 から遠いほど元の値を残す」ブレンドをしている（`confidence`）。
//! 注意: p が 0〜1 だと confidence は最大でも 0.25^20 ≈ 1e-12 なので、このブレンドは実質効いておらず、
//! 出力はほぼガイデッドフィルタの結果そのもの。確信度の高い画素を残したいなら式の見直しが必要。
//!
//! 注意: 4 枚の積分画像（f32 × (H+1)(W+1)）を作るので、8000x8000 だと一時的に約1GBを使う。
//! また積分画像を f32 で累積しているため、大きな画像では精度が落ちる。

use image::RgbaImage;
use ndarray::{Array4, ArrayView4};
use rayon::{
    iter::{IndexedParallelIterator, ParallelIterator},
    slice::ParallelSliceMut,
};

use crate::image::image::image::Image;

pub struct SAM2MaskSmoother;

impl SAM2MaskSmoother {
    pub fn smoothing(mask: ArrayView4<f32>, original_image: &Image) -> Array4<f32> {
        let radius = 16usize;

        assert_eq!(mask.shape()[0], 1);
        assert_eq!(mask.shape()[1], 1);

        let height = mask.shape()[2];
        let width = mask.shape()[3];

        assert_eq!(height as u16, original_image.image_size().height());
        assert_eq!(width as u16, original_image.image_size().width());

        let mask_2d = mask.slice(ndarray::s![0, 0, .., ..]);
        let mask_slice = mask_2d.as_slice().expect("Mask must be contiguous");

        let guide = Self::build_guidance_image(original_image);
        let mask_values = mask_slice.to_vec();

        let integral_i = Self::build_integral(&guide, height, width);
        let integral_p = Self::build_integral(&mask_values, height, width);
        let integral_ip = Self::build_integral(
            &mask_values
                .iter()
                .zip(guide.iter())
                .map(|(mask_value, guide_value)| mask_value * guide_value)
                .collect::<Vec<_>>(),
            height,
            width,
        );
        let integral_i2 = Self::build_integral(
            &guide.iter().map(|value| value * value).collect::<Vec<_>>(),
            height,
            width,
        );

        let eps = 1e-1f32;
        let mut output = Array4::<f32>::zeros((1, 1, height, width));
        let output_slice = output.as_slice_mut().unwrap();

        output_slice
            .par_chunks_exact_mut(width)
            .enumerate()
            .for_each(|(y, row)| {
                let y0 = y.saturating_sub(radius);
                let y1 = (y + radius).min(height - 1);

                for x in 0..width {
                    let x0 = x.saturating_sub(radius);
                    let x1 = (x + radius).min(width - 1);

                    let area = ((x1 - x0 + 1) * (y1 - y0 + 1)) as f32;

                    let mean_i = Self::rect_sum(&integral_i, width, x0, y0, x1, y1) / area;
                    let mean_p = Self::rect_sum(&integral_p, width, x0, y0, x1, y1) / area;
                    let mean_ip = Self::rect_sum(&integral_ip, width, x0, y0, x1, y1) / area;
                    let mean_i2 = Self::rect_sum(&integral_i2, width, x0, y0, x1, y1) / area;

                    let var_i = mean_i2 - mean_i * mean_i;
                    let cov_ip = mean_ip - mean_i * mean_p;

                    let a = cov_ip / (var_i + eps);

                    let b = mean_p - a * mean_i;
                    let guide_index = y * width + x;

                    let filtered = (a * guide[guide_index] + b).clamp(0.0, 1.0);

                    let original_p = mask_values[guide_index];
                    let confidence = ((original_p - 0.5).abs() * 0.5).powi(20);

                    row[x] = filtered * (1.0 - confidence) + original_p * confidence;
                }
            });

        output
    }

    /// 元画像 RGBA → 輝度（ITU-R BT.601 の係数）0〜1 のグレースケール。
    fn build_guidance_image(original_image: &Image) -> Vec<f32> {
        let width = original_image.image_size().width() as u32;
        let height = original_image.image_size().height() as u32;

        let image = RgbaImage::from_raw(
            width,
            height,
            original_image.image_data().clone().into_image(),
        )
        .expect("Rgba Image generate failed");

        image
            .pixels()
            .map(|pixel| {
                let r = pixel[0] as f32 / 255.0;
                let g = pixel[1] as f32 / 255.0;
                let b = pixel[2] as f32 / 255.0;

                0.299 * r + 0.587 * g + 0.114 * b
            })
            .collect()
    }

    /// 積分画像を作る。サイズは (H+1) x (W+1) で、先頭行・先頭列は 0（境界処理を省くため）。
    fn build_integral(values: &[f32], height: usize, width: usize) -> Vec<f32> {
        let stride = width + 1;
        let mut integral = vec![0.0; (height + 1) * stride];

        for y in 0..height {
            for x in 0..width {
                let index = (y + 1) * stride + (x + 1);
                let value = values[y * width + x];

                integral[index] =
                    value + integral[(y + 1) * stride + x] + integral[y * stride + (x + 1)]
                        - integral[y * stride + x];
            }
        }

        integral
    }

    /// 積分画像から矩形 [x0..=x1] x [y0..=y1] の総和を O(1) で求める。
    const fn rect_sum(
        integral: &[f32],
        width: usize,
        x0: usize,
        y0: usize,
        x1: usize,
        y1: usize,
    ) -> f32 {
        let stride = width + 1;

        integral[(y1 + 1) * stride + (x1 + 1)]
            - integral[(y1 + 1) * stride + x0]
            - integral[y0 * stride + (x1 + 1)]
            + integral[y0 * stride + x0]
    }
}
