//! プレビューの前処理・後処理の純粋な計算（本番のワーカーと同じ数値にする）。
//!
//! - `resize_bilinear_rgb8`: `cv2.resize(uint8 の RGB, INTER_LINEAR)` と同じ結果（`tracker.py` の `to_pixel_values`）。
//!   OpenCV の uint8 の計算（係数 11 bit の固定小数点、SIMD 版の丸め、縦は端でも重みを 0 にしない）をなぞってある。
//! - `resize_bilinear_f32`: `cv2.resize(float32, INTER_LINEAR)` と同じ結果（`compositor.py` の `alpha_from_logits`）。
//!   重みは f64 で求めて f32 にし、補間は横 → 縦の順に f32 で計算する（誤差は f32 の丸め程度）。
//! - `alpha_from_logits`: `worker/cutout/compositor.py` の `alpha_from_logits` と同じ
//!   （バイリニア拡大 → ±50 にクリップ → ×`EDGE_SHARPNESS` → ±50 にクリップ → sigmoid）
//!
//! どちらも画素の中心を合わせる座標（`(dst + 0.5) * src / dst - 0.5`）で、縮小でもアンチエイリアスしない。
//! 照合の方法: Python（OpenCV 5.0）で同じ入力を計算し、uint8 は完全一致、float32 は 4e-6 以内を確かめた（テストの期待値も同じ方法で作った）。

use image::{GrayImage, RgbImage};
use ndarray::Array2;

/// alpha を硬くする鋭さ。`worker/cutout/compositor.py` の `EDGE_SHARPNESS` と同じ値にすること。
pub const EDGE_SHARPNESS: f32 = 16.0;

/// ロジットをクリップする幅（`exp` の溢れを避ける。ワーカーと同じ）。
const LOGIT_CLIP: f32 = 50.0;

/// OpenCV の uint8 の補間係数の倍率（`INTER_RESIZE_COEF_SCALE`）。
const COEF_SCALE: f32 = 2048.0;

/// uint8 用の 1 軸ぶんの表。`(左の添字, 右の添字, 左の重み, 右の重み)`（重みは 2048 倍の整数）。
/// OpenCV と同じく位置は f32 で求める。横（`clamp_weight = true`）は端で重みを 0 にし、
/// 縦（`false`）は添字だけを端に寄せて重みはそのまま使う（OpenCV の `resize` の実装がそうなっている）。
fn u8_taps(src: usize, dst: usize, clamp_weight: bool) -> Vec<(usize, usize, i32, i32)> {
    let scale = 1.0 / (dst as f64 / src as f64);
    let last = src as i64 - 1;
    (0..dst)
        .map(|d| {
            let f = ((d as f64 + 0.5) * scale - 0.5) as f32;
            let mut i = f.floor() as i64;
            let mut frac = f - i as f32;
            if clamp_weight {
                if i < 0 {
                    i = 0;
                    frac = 0.0;
                }
                if i >= last {
                    i = last;
                    frac = 0.0;
                }
            }
            let w0 = ((1.0 - frac) * COEF_SCALE).round_ties_even() as i32;
            let w1 = (frac * COEF_SCALE).round_ties_even() as i32;
            (
                i.clamp(0, last) as usize,
                (i + 1).clamp(0, last) as usize,
                w0,
                w1,
            )
        })
        .collect()
}

/// f32 用の 1 軸ぶんの表。`(左の添字, 右の添字, 右の重み)`。重みは f64 で求めてから f32 にする。
fn f32_taps(src: usize, dst: usize) -> Vec<(usize, usize, f32)> {
    let scale = src as f64 / dst as f64;
    let last = src as i64 - 1;
    (0..dst)
        .map(|d| {
            let f = (d as f64 + 0.5) * scale - 0.5;
            let mut i = f.floor() as i64;
            let mut frac = f - i as f64;
            if i < 0 {
                i = 0;
                frac = 0.0;
            }
            if i >= last {
                i = last;
                frac = 0.0;
            }
            (i as usize, (i + 1).min(last) as usize, frac as f32)
        })
        .collect()
}

/// HWC に並んだ f32 の画像（`channels` チャンネル）をバイリニアで拡大縮小する。
/// 横 → 縦の順に補間する（OpenCV と同じ順番なので f32 の丸めもほぼ揃う）。
pub fn resize_bilinear_f32(
    src: &[f32],
    src_w: usize,
    src_h: usize,
    channels: usize,
    dst_w: usize,
    dst_h: usize,
) -> Vec<f32> {
    assert_eq!(src.len(), src_w * src_h * channels);
    let xs = f32_taps(src_w, dst_w);
    let ys = f32_taps(src_h, dst_h);

    // 先に全行を横方向に補間しておく（縦の補間で同じ行を何度も使うため）
    let stride = dst_w * channels;
    let mut rows = vec![0.0f32; src_h * stride];
    for y in 0..src_h {
        let src_row = &src[y * src_w * channels..(y + 1) * src_w * channels];
        let row = &mut rows[y * stride..(y + 1) * stride];
        for (dx, &(x0, x1, fx)) in xs.iter().enumerate() {
            for c in 0..channels {
                row[dx * channels + c] =
                    src_row[x0 * channels + c] * (1.0 - fx) + src_row[x1 * channels + c] * fx;
            }
        }
    }

    let mut out = vec![0.0f32; dst_h * stride];
    for (dy, &(y0, y1, fy)) in ys.iter().enumerate() {
        let r0 = &rows[y0 * stride..(y0 + 1) * stride];
        let r1 = &rows[y1 * stride..(y1 + 1) * stride];
        let o = &mut out[dy * stride..(dy + 1) * stride];
        for i in 0..stride {
            o[i] = r0[i] * (1.0 - fy) + r1[i] * fy;
        }
    }
    out
}

/// RGB の画像をバイリニアで拡大縮小する（`cv2.resize` に uint8 を渡したときと同じ整数の計算）。
pub fn resize_bilinear_rgb8(src: &RgbImage, dst_w: u32, dst_h: u32) -> RgbImage {
    const CH: usize = 3;
    let (src_w, src_h) = (src.width() as usize, src.height() as usize);
    let (dst_w_us, dst_h_us) = (dst_w as usize, dst_h as usize);
    let raw = src.as_raw();
    let xs = u8_taps(src_w, dst_w_us, true);
    let ys = u8_taps(src_h, dst_h_us, false);

    // 横: 画素 × 重み（2048 倍）の整数
    let stride = dst_w_us * CH;
    let mut rows = vec![0i32; src_h * stride];
    for y in 0..src_h {
        let src_row = &raw[y * src_w * CH..(y + 1) * src_w * CH];
        let row = &mut rows[y * stride..(y + 1) * stride];
        for (dx, &(x0, x1, w0, w1)) in xs.iter().enumerate() {
            for c in 0..CH {
                row[dx * CH + c] =
                    src_row[x0 * CH + c] as i32 * w0 + src_row[x1 * CH + c] as i32 * w1;
            }
        }
    }

    // 縦: OpenCV の SIMD 版（VResizeLinearVec_32s8u）と同じ丸め
    let mut out = vec![0u8; dst_h_us * stride];
    for (dy, &(y0, y1, w0, w1)) in ys.iter().enumerate() {
        let r0 = &rows[y0 * stride..(y0 + 1) * stride];
        let r1 = &rows[y1 * stride..(y1 + 1) * stride];
        let o = &mut out[dy * stride..(dy + 1) * stride];
        for i in 0..stride {
            let v = ((((r0[i] >> 4) * w0) >> 16) + (((r1[i] >> 4) * w1) >> 16) + 2) >> 2;
            o[i] = v.clamp(0, 255) as u8;
        }
    }
    RgbImage::from_raw(dst_w, dst_h, out).expect("大きさは dst_w x dst_h x 3")
}

/// ロジット（任意の大きさ）を `width` x `height` の alpha（0〜1、行優先）にする。
pub fn alpha_from_logits(logits: &Array2<f32>, width: u32, height: u32) -> Vec<f32> {
    let (src_h, src_w) = logits.dim();
    let flat: Vec<f32> = logits.iter().copied().collect();
    let up = resize_bilinear_f32(&flat, src_w, src_h, 1, width as usize, height as usize);
    up.into_iter()
        .map(|v| {
            let v = v.clamp(-LOGIT_CLIP, LOGIT_CLIP);
            let sharp = (EDGE_SHARPNESS * v).clamp(-LOGIT_CLIP, LOGIT_CLIP);
            1.0 / (1.0 + (-sharp).exp())
        })
        .collect()
}

/// alpha（0〜1）をグレースケール 8bit の画像にする（255 = 前景）。
pub fn alpha_to_gray(alpha: &[f32], width: u32, height: u32) -> GrayImage {
    let bytes = alpha
        .iter()
        .map(|&a| (a * 255.0).round().clamp(0.0, 255.0) as u8)
        .collect();
    GrayImage::from_raw(width, height, bytes).expect("大きさは width x height")
}

#[cfg(test)]
mod image_math_test {
    use super::*;

    /// 期待値は Python で `cv2.resize(img, (3, 3), interpolation=cv2.INTER_LINEAR)` を実行して得たもの。
    fn sample_rgb() -> RgbImage {
        let px: [[u8; 3]; 8] = [
            [0, 10, 200],
            [50, 60, 70],
            [255, 0, 128],
            [30, 30, 30],
            [100, 110, 120],
            [5, 250, 9],
            [77, 88, 99],
            [200, 100, 0],
        ];
        RgbImage::from_raw(4, 2, px.concat()).unwrap()
    }

    #[test]
    fn rgb_resize_matches_opencv_exactly() {
        let cases: [(u32, u32, Vec<[u8; 3]>); 2] = [
            (
                3,
                3,
                vec![
                    [8, 18, 178],
                    [152, 30, 99],
                    [67, 25, 46],
                    [46, 76, 140],
                    [97, 100, 77],
                    [123, 62, 31],
                    [84, 133, 101],
                    [41, 169, 54],
                    [179, 98, 16],
                ],
            ),
            (
                7,
                5,
                vec![
                    [0, 10, 200],
                    [18, 28, 154],
                    [46, 56, 79],
                    [152, 30, 99],
                    [239, 2, 121],
                    [110, 19, 65],
                    [30, 30, 30],
                    [10, 20, 192],
                    [23, 41, 146],
                    [43, 75, 73],
                    [141, 44, 94],
                    [224, 11, 118],
                    [115, 27, 62],
                    [47, 37, 27],
                    [50, 60, 160],
                    [42, 94, 117],
                    [29, 148, 48],
                    [97, 100, 77],
                    [162, 45, 106],
                    [133, 57, 50],
                    [115, 65, 15],
                    [90, 100, 128],
                    [61, 147, 88],
                    [15, 221, 23],
                    [52, 155, 58],
                    [101, 80, 95],
                    [151, 88, 38],
                    [183, 93, 3],
                    [100, 110, 120],
                    [66, 160, 80],
                    [12, 240, 17],
                    [41, 169, 54],
                    [86, 89, 92],
                    [156, 96, 35],
                    [200, 100, 0],
                ],
            ),
        ];
        for (w, h, expected) in cases {
            let out = resize_bilinear_rgb8(&sample_rgb(), w, h);
            assert_eq!(out.dimensions(), (w, h));
            let got: Vec<[u8; 3]> = out.pixels().map(|p| p.0).collect();
            assert_eq!(got, expected, "{w}x{h}");
        }
    }

    #[test]
    fn taps_hand_computed_example() {
        // 2 → 4 に拡大: 位置 = (d + 0.5) * 0.5 - 0.5 = -0.25, 0.25, 0.75, 1.25
        // 端は端の画素（重み 0）、中は線形補間
        assert_eq!(
            f32_taps(2, 4),
            vec![(0, 1, 0.0), (0, 1, 0.25), (0, 1, 0.75), (1, 1, 0.0)]
        );
        let out = resize_bilinear_f32(&[0.0, 4.0], 2, 1, 1, 4, 1);
        assert_eq!(out, vec![0.0, 1.0, 3.0, 4.0]);
        // uint8 の横は同じ（重みは 2048 倍）。縦は端でも重みを 0 にせず、添字だけを端に寄せる
        assert_eq!(
            u8_taps(2, 4, true),
            vec![
                (0, 1, 2048, 0),
                (0, 1, 1536, 512),
                (0, 1, 512, 1536),
                (1, 1, 2048, 0)
            ]
        );
        assert_eq!(
            u8_taps(2, 4, false),
            vec![
                (0, 0, 512, 1536),
                (0, 1, 1536, 512),
                (0, 1, 512, 1536),
                (1, 1, 1536, 512)
            ]
        );
    }

    /// 期待値は Python で `alpha_from_logits`（compositor.py）と同じ式を実行して得たもの。
    #[test]
    fn alpha_matches_worker_formula() {
        let logits = Array2::from_shape_vec((2, 2), vec![-1024.0, 0.1, 0.05, 3.0]).unwrap();
        let alpha = alpha_from_logits(&logits, 5, 3);
        let tiny = 1.928_749_9e-22_f32;
        let expected = [
            tiny,
            tiny,
            tiny,
            tiny,
            0.832_018_4,
            tiny,
            tiny,
            tiny,
            tiny,
            1.0,
            0.689_974_5,
            0.996_010_2,
            1.0,
            1.0,
            1.0,
        ];
        assert_eq!(alpha.len(), expected.len());
        for (i, (got, want)) in alpha.iter().zip(expected).enumerate() {
            assert!(
                (got - want).abs() <= 1e-6 * want.max(1e-20) + 1e-27,
                "{i}: {got} vs {want}"
            );
        }
    }

    #[test]
    fn alpha_to_gray_rounds_to_bytes() {
        let img = alpha_to_gray(&[0.0, 0.5, 1.0, 0.002], 2, 2);
        assert_eq!(img.as_raw(), &vec![0, 128, 255, 1]);
    }
}
