//! プレビューの前処理・後処理の純粋な計算（本番のワーカーと同じ数値にする）。
//!
//! - `resize_bilinear_rgb8`: `cv2.resize(uint8 の RGB, INTER_LINEAR)` と同じ結果（`tracker.py` の `to_pixel_values`）。
//!   OpenCV の uint8 の計算（係数 11 bit の固定小数点、SIMD 版の丸め、縦は端でも重みを 0 にしない）をなぞってある。
//! - `resize_bilinear_f32`: `cv2.resize(float32, INTER_LINEAR)` と同じ結果（`compositor.py` の `alpha_from_logits`）。
//!   重みは f64 で求めて f32 にし、補間は横 → 縦の順に f32 で計算する（誤差は f32 の丸め程度）。
//! - `alpha_from_logits`: `worker/cutout/compositor.py` の `alpha_from_logits` と同じ
//!   （バイリニア拡大 → ±50 にクリップ → ×`EDGE_SHARPNESS` → ±50 にクリップ → sigmoid）
//! - `shrink_edge`: `compositor.py` の `shrink_edge` と同じ（ガウスぼかし（端は BORDER_REFLECT_101）→ 縁を内側へずらす）。
//!   ぼかしは OpenCV と同じく窓の重みを f64 で求めて f32 にし、横 → 縦の順に f32 で足す
//! - `keep_largest_region`: `worker/cutout/postprocess.py` の `keep_largest_region` と同じ
//!   （見える画素を 8 近傍でつなぎ、いちばん大きい塊だけ残す。同じ大きさなら上・左で先に見つかった塊）
//!
//! どちらも画素の中心を合わせる座標（`(dst + 0.5) * src / dst - 0.5`）で、縮小でもアンチエイリアスしない。
//! 照合の方法: Python（OpenCV 5.0）で同じ入力を計算し、uint8 は完全一致、float32 は 4e-6 以内を確かめた（テストの期待値も同じ方法で作った）。

use image::{GrayImage, RgbImage};
use ndarray::Array2;

/// alpha を硬くする鋭さ。`worker/cutout/compositor.py` の `EDGE_SHARPNESS` と同じ値にすること。
pub const EDGE_SHARPNESS: f32 = 16.0;

/// 縁を滑らかにするガウスぼかしの σ と、縁を内側へ縮める量（出力の画素）。
/// `worker/cutout/compositor.py` の `EDGE_SMOOTH_SIGMA_PX`・`EDGE_SHRINK_PX` と同じ値にすること。
pub const EDGE_SMOOTH_SIGMA_PX: f64 = 1.5;
pub const EDGE_SHRINK_PX: f64 = 2.0;

/// ワーカーの出力の長辺の上限（`worker/cutout/video_io.py` の `MAX_LONG_SIDE`）。
const OUTPUT_MAX_LONG_SIDE: u32 = 1080;

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

/// プレビューの画像（スマホが送ったフレームの大きさ）で、出力と同じ見た目に縮めるための倍率。
/// 出力は長辺 1080 以下に縮小されるので、それより大きいフレームでは縮める量・ぼかしを同じ割合で大きくする。
pub fn preview_px_scale(width: u32, height: u32) -> f64 {
    let long = width.max(height).max(1);
    long as f64 / long.min(OUTPUT_MAX_LONG_SIDE) as f64
}

/// erf（f64）。`|x|` が小さい（この用途では約 0.94）ので Taylor 級数で十分（40 項で f64 の精度）。
fn erf(x: f64) -> f64 {
    let (mut term, mut sum) = (x, x);
    for n in 1..40 {
        term *= -x * x / n as f64;
        sum += term / (2 * n + 1) as f64;
    }
    sum * 2.0 / std::f64::consts::PI.sqrt()
}

/// OpenCV の `getGaussianKernel(ksize, sigma, CV_32F)` と同じ重み（ksize = 2⌈3σ⌉ + 1。ワーカーの `gaussian_ksize`）。
fn gaussian_kernel(sigma: f64) -> Vec<f32> {
    let k = 2 * (3.0 * sigma).ceil() as usize + 1;
    let c = (k - 1) as f64 / 2.0;
    let scale = -0.5 / (sigma * sigma);
    let w: Vec<f64> = (0..k).map(|i| (scale * (i as f64 - c).powi(2)).exp()).collect();
    let sum: f64 = w.iter().sum();
    w.into_iter().map(|v| (v / sum) as f32).collect()
}

/// OpenCV の BORDER_REFLECT_101（`gfedcb|abcdefgh|gfedcba`）。
fn reflect101(i: isize, n: usize) -> usize {
    let n = n as isize;
    if n == 1 {
        return 0;
    }
    let mut i = i;
    loop {
        if i < 0 {
            i = -i;
        } else if i >= n {
            i = 2 * n - 2 - i;
        } else {
            return i as usize;
        }
    }
}

/// alpha の縁を `EDGE_SHRINK_PX` × `px_scale` だけ内側へ縮め、ギザギザを滑らかにする（`compositor.py` の `shrink_edge`）。
/// 出力のフレームでは `px_scale = 1`、プレビューでは `preview_px_scale`。
pub fn shrink_edge(alpha: &[f32], width: u32, height: u32, px_scale: f64) -> Vec<f32> {
    let (w, h) = (width as usize, height as usize);
    let sigma = EDGE_SMOOTH_SIGMA_PX * px_scale;
    let z = EDGE_SHRINK_PX / EDGE_SMOOTH_SIGMA_PX;
    let level = (0.5 * (1.0 + erf(z / std::f64::consts::SQRT_2))) as f32;
    let pdf = (-0.5 * z * z).exp() / (2.0 * std::f64::consts::PI).sqrt();
    let gain = (sigma / pdf) as f32;
    let kernel = gaussian_kernel(sigma);
    let r = (kernel.len() / 2) as isize;

    let mut tmp = vec![0f32; w * h];
    for y in 0..h {
        let row = &alpha[y * w..(y + 1) * w];
        for x in 0..w {
            let mut acc = 0f32;
            for (j, &k) in kernel.iter().enumerate() {
                acc += k * row[reflect101(x as isize + j as isize - r, w)];
            }
            tmp[y * w + x] = acc;
        }
    }
    let mut out = vec![0f32; w * h];
    for y in 0..h {
        for x in 0..w {
            let mut acc = 0f32;
            for (j, &k) in kernel.iter().enumerate() {
                acc += k * tmp[reflect101(y as isize + j as isize - r, h) * w + x];
            }
            out[y * w + x] = (0.5 + (acc - level) * gain).clamp(0.0, 1.0);
        }
    }
    out
}

/// これより大きい alpha は 8bit にすると 1 以上（目に見える）。`postprocess.py` の `VISIBLE_ALPHA` と同じ。
const VISIBLE_ALPHA: f32 = 0.5 / 255.0;

/// 見える画素（`VISIBLE_ALPHA` より大きい）を 8 近傍でつないだ塊のうち、いちばん大きいもの以外の alpha を 0 にする
/// （`worker/cutout/postprocess.py` の `keep_largest_region`。体から離れた点や隣の人の切れ端を消す）。
pub fn keep_largest_region(alpha: &mut [f32], width: u32, height: u32) {
    let (w, h) = (width as usize, height as usize);
    let mut labels = vec![0u32; w * h]; // 0 = 見えない画素。塊はラスター順に 1, 2, ... と番号を振る
    let mut areas: Vec<usize> = vec![0];
    let mut stack = Vec::new();
    for start in 0..w * h {
        if labels[start] != 0 || alpha[start] <= VISIBLE_ALPHA {
            continue;
        }
        let label = areas.len() as u32;
        let mut area = 0;
        labels[start] = label;
        stack.push(start);
        while let Some(i) = stack.pop() {
            area += 1;
            let (x, y) = ((i % w) as isize, (i / w) as isize);
            for dy in -1..=1 {
                for dx in -1..=1 {
                    let (nx, ny) = (x + dx, y + dy);
                    if nx < 0 || ny < 0 || nx >= w as isize || ny >= h as isize {
                        continue;
                    }
                    let j = ny as usize * w + nx as usize;
                    if labels[j] == 0 && alpha[j] > VISIBLE_ALPHA {
                        labels[j] = label;
                        stack.push(j);
                    }
                }
            }
        }
        areas.push(area);
    }
    if areas.len() <= 2 {
        return; // 塊が 0 個か 1 個
    }
    // 同じ大きさなら先に見つかった（番号の小さい）塊を残す（numpy の argmax と同じ）
    let mut largest = 1;
    for (label, &area) in areas.iter().enumerate().skip(2) {
        if area > areas[largest] {
            largest = label;
        }
    }
    for (a, &l) in alpha.iter_mut().zip(&labels) {
        if l != 0 && l as usize != largest {
            *a = 0.0;
        }
    }
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

    /// 期待値は Python で `shrink_edge`（compositor.py。OpenCV の GaussianBlur）を実行して得たもの。
    #[test]
    fn shrink_edge_matches_worker() {
        let (w, h) = (20usize, 12usize);
        let mut alpha: Vec<f32> = (0..h)
            .flat_map(|y| (0..w).map(move |x| if x + (y / 3) % 2 * 2 >= 8 { 1.0 } else { 0.0 }))
            .collect();
        alpha[5 * w + 15] = 0.4;
        let cases: [(f64, &[(usize, usize, f32)]); 2] = [
            (
                1.0,
                &[
                    (0, 8, 0.0),
                    (0, 9, 0.044_512_033),
                    (0, 10, 0.961_619_6),
                    (0, 11, 1.0),
                    (2, 9, 0.403_648_1),
                    (4, 8, 0.021_066_487),
                    (4, 9, 0.847_224_4),
                    (11, 8, 0.662_888_4),
                    (5, 15, 0.945_892_6),
                    (6, 0, 0.0),
                ],
            ),
            (
                1.5,
                &[
                    (0, 10, 0.324_689_15),
                    (2, 10, 0.510_145_96),
                    (4, 10, 0.718_340_5),
                    (11, 9, 0.297_125_43),
                    (5, 15, 1.0),
                ],
            ),
        ];
        for (scale, points) in cases {
            let out = shrink_edge(&alpha, w as u32, h as u32, scale);
            for &(y, x, want) in points {
                let got = out[y * w + x];
                assert!((got - want).abs() <= 2e-5, "scale {scale} ({y},{x}): {got} vs {want}");
            }
        }
    }

    fn region_alpha(w: usize, h: usize, boxes: &[(usize, usize, usize, usize, f32)]) -> Vec<f32> {
        let mut a = vec![0f32; w * h];
        for &(y0, y1, x0, x1, v) in boxes {
            for y in y0..y1 {
                for x in x0..x1 {
                    a[y * w + x] = v;
                }
            }
        }
        a
    }

    #[test]
    fn keep_largest_region_removes_everything_else() {
        // 体（10x4）、離れた塊、かすかな塊（0.05）、斜めに接する塊
        let (w, h) = (20, 12);
        let mut a = region_alpha(
            w,
            h,
            &[(2, 12, 5, 9, 1.0), (0, 2, 15, 18, 1.0), (5, 7, 12, 14, 0.05), (0, 2, 9, 11, 1.0)],
        );
        keep_largest_region(&mut a, w as u32, h as u32);
        assert!(a[2 * w + 5] == 1.0 && a[11 * w + 8] == 1.0); // 体
        assert_eq!(a[w + 10], 1.0); // (1,9)〜 は体の角 (2,8) と斜めに接する → 残る
        assert_eq!(a[16], 0.0); // 離れた塊
        assert_eq!(a[6 * w + 12], 0.0); // かすかでも見える塊は消す
    }

    #[test]
    fn keep_largest_region_tie_keeps_the_first_found() {
        let (w, h) = (10, 3);
        let mut a = region_alpha(w, h, &[(0, 1, 6, 8, 1.0), (2, 3, 0, 2, 1.0)]);
        keep_largest_region(&mut a, w as u32, h as u32);
        assert_eq!(a[6], 1.0); // ラスター順で先（上）の塊
        assert_eq!(a[2 * w], 0.0);
    }

    #[test]
    fn preview_px_scale_follows_output_size() {
        assert_eq!(preview_px_scale(576, 1024), 1.0); // 出力と同じ大きさ
        assert_eq!(preview_px_scale(1080, 1920), 1920.0 / 1080.0); // 出力は長辺 1080 に縮む
    }

    #[test]
    fn alpha_to_gray_rounds_to_bytes() {
        let img = alpha_to_gray(&[0.0, 0.5, 1.0, 0.002], 2, 2);
        assert_eq!(img.as_raw(), &vec![0, 128, 255, 1]);
    }
}
