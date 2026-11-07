//! SAM2.1 tiny（`diffusionstudio/sam2.1-tiny-video-onnx-fp16`、本番のワーカーと同じリビジョン）でプレビューを出す実装。
//!
//! モデルの置き場所（`models/sam2.1-tiny-video/`。git 管理外）:
//! - `constants.json`（`image_mean` / `image_std`）
//! - `onnx/vision_encoder.onnx`: `pixel_values` [1,3,512,512] → `feats0` [1,32,128,128], `feats1` [1,64,64,64],
//!   `feats2_no_mem` [1,256,32,32] ほか
//! - `onnx/mask_decoder.onnx`: `feats0`, `feats1`, `feats2_cond`, `input_points` [1,1,N,2], `input_labels` [1,1,N]
//!   → `high_res_mask` [1,1,512,512] ほか
//!
//! 計算は `worker/cutout/tracker.py` の 1 枚目と同じ（`to_pixel_values`・点の変換）。
//! 入力の型はモデルの宣言に合わせ（fp16 なら変換）、fp16 の出力は f32 にする（`worker/cutout/models.py` の `OnnxGraph` と同じ）。
//!
//! スレッド安全性: グラフごとに `Mutex<Session>`（画像の `Sam2Segmenter` と同じ。プレビューは直列に動く）。

use std::{collections::HashMap, fs, path::Path, sync::Mutex};

use half::f16;
use image::RgbImage;
use ndarray::{Array2, Array3, Array4, ArrayD, Ix4};
use ort::{
    session::{Session, SessionOutputs, builder::GraphOptimizationLevel},
    value::{DynTensor, Tensor, TensorElementType},
};

use crate::video_cutout::{
    job::point::{PointLabel, Points},
    preview::{
        error::PreviewError,
        image_math::resize_bilinear_rgb8,
        segmenter::{PREVIEW_INPUT_SIZE, PreviewFeatures, PreviewSegmenter},
    },
};

/// モデルのディレクトリの中で必要なファイル（`config.rs` の起動時の確認にも使う）。
pub const PREVIEW_MODEL_FILES: [&str; 3] = [
    "constants.json",
    "onnx/vision_encoder.onnx",
    "onnx/mask_decoder.onnx",
];

/// フレームを 512x512 にバイリニアで引き伸ばし（余白なし）、0〜1 → mean/std で正規化した CHW `[1,3,512,512]`。
/// `tracker.py` の `to_pixel_values` と同じ。
pub fn to_pixel_values(frame: &RgbImage, mean: [f32; 3], std: [f32; 3]) -> Array4<f32> {
    let s = PREVIEW_INPUT_SIZE;
    let resized = resize_bilinear_rgb8(frame, s as u32, s as u32);
    let raw = resized.as_raw();
    let mut out = Array4::<f32>::zeros((1, 3, s, s));
    for y in 0..s {
        for x in 0..s {
            let base = (y * s + x) * 3;
            for c in 0..3 {
                out[[0, c, y, x]] = (raw[base + c] as f32 / 255.0 - mean[c]) / std[c];
            }
        }
    }
    out
}

/// 点をデコーダの入力にする。座標は 0〜1 → ×512（`[1,1,N,2]`）、ラベルは include=1 / exclude=0（`[1,1,N]`）。詰め点なし。
pub fn point_inputs(points: &Points) -> (Array4<f32>, Array3<i32>) {
    let s = PREVIEW_INPUT_SIZE as f32;
    let pts = points.as_slice();
    let mut coords = Array4::<f32>::zeros((1, 1, pts.len(), 2));
    let mut labels = Array3::<i32>::zeros((1, 1, pts.len()));
    for (i, p) in pts.iter().enumerate() {
        coords[[0, 0, i, 0]] = p.x() * s;
        coords[[0, 0, i, 1]] = p.y() * s;
        labels[[0, 0, i]] = match p.label() {
            PointLabel::Include => 1,
            PointLabel::Exclude => 0,
        };
    }
    (coords, labels)
}

pub struct Sam2TinyPreviewSegmenter {
    vision_encoder: OnnxGraph,
    mask_decoder: OnnxGraph,
    image_mean: [f32; 3],
    image_std: [f32; 3],
}

#[derive(serde::Deserialize)]
struct Constants {
    image_mean: [f32; 3],
    image_std: [f32; 3],
}

impl Sam2TinyPreviewSegmenter {
    pub fn new(model_dir: &Path) -> Result<Self, PreviewError> {
        let constants_path = model_dir.join("constants.json");
        let text = fs::read_to_string(&constants_path)
            .map_err(|e| PreviewError::ModelLoad(format!("{}: {e}", constants_path.display())))?;
        let constants: Constants = serde_json::from_str(&text)
            .map_err(|e| PreviewError::ModelLoad(format!("{}: {e}", constants_path.display())))?;
        Ok(Self {
            vision_encoder: OnnxGraph::load(&model_dir.join("onnx/vision_encoder.onnx"))?,
            mask_decoder: OnnxGraph::load(&model_dir.join("onnx/mask_decoder.onnx"))?,
            image_mean: constants.image_mean,
            image_std: constants.image_std,
        })
    }
}

impl PreviewSegmenter for Sam2TinyPreviewSegmenter {
    fn encode(&self, frame: &RgbImage) -> Result<PreviewFeatures, PreviewError> {
        let pixel_values = to_pixel_values(frame, self.image_mean, self.image_std);
        let mut out = self.vision_encoder.run(
            vec![("pixel_values", Feed::F32(pixel_values.into_dyn()))],
            &["feats0", "feats1", "feats2_no_mem"],
        )?;
        let mut take = |name: &str| -> Result<Array4<f32>, PreviewError> {
            out.remove(name)
                .expect("run が名前を全部返す")
                .into_dimensionality::<Ix4>()
                .map_err(|e| PreviewError::Inference(format!("{name}: {e}")))
        };
        Ok(PreviewFeatures {
            feats0: take("feats0")?,
            feats1: take("feats1")?,
            feats2_no_mem: take("feats2_no_mem")?,
            frame_width: frame.width(),
            frame_height: frame.height(),
        })
    }

    fn decode(
        &self,
        features: &PreviewFeatures,
        points: &Points,
    ) -> Result<Array2<f32>, PreviewError> {
        let (coords, labels) = point_inputs(points);
        let mut out = self.mask_decoder.run(
            vec![
                ("feats0", Feed::F32(features.feats0.clone().into_dyn())),
                ("feats1", Feed::F32(features.feats1.clone().into_dyn())),
                (
                    "feats2_cond",
                    Feed::F32(features.feats2_no_mem.clone().into_dyn()),
                ),
                ("input_points", Feed::F32(coords.into_dyn())),
                ("input_labels", Feed::I32(labels.into_dyn())),
            ],
            &["high_res_mask"],
        )?;
        let mask = out.remove("high_res_mask").expect("run が名前を全部返す");
        let s = PREVIEW_INPUT_SIZE;
        if mask.len() != s * s {
            return Err(PreviewError::Inference(format!(
                "high_res_mask の形が想定外: {:?}",
                mask.shape()
            )));
        }
        Array2::from_shape_vec((s, s), mask.into_iter().collect())
            .map_err(|e| PreviewError::Inference(e.to_string()))
    }
}

/// グラフに渡す値（f32 か i32 で作り、モデルの宣言に合わせて変換する）。
enum Feed {
    F32(ArrayD<f32>),
    I32(ArrayD<i32>),
}

/// 1 つの ONNX グラフ。入力の型を読み込み時に覚えておく。
struct OnnxGraph {
    name: String,
    session: Mutex<Session>,
    input_types: HashMap<String, TensorElementType>,
}

impl OnnxGraph {
    fn load(path: &Path) -> Result<Self, PreviewError> {
        let load_err = |e: ort::Error| PreviewError::ModelLoad(format!("{}: {e}", path.display()));
        let session = Session::builder()
            .map_err(load_err)?
            .with_optimization_level(GraphOptimizationLevel::Level3)
            .map_err(|e| load_err(e.into()))?
            .commit_from_file(path)
            .map_err(load_err)?;
        let input_types = session
            .inputs()
            .iter()
            .filter_map(|i| Some((i.name().to_string(), i.dtype().tensor_type()?)))
            .collect();
        Ok(Self {
            name: path
                .file_stem()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_default(),
            session: Mutex::new(session),
            input_types,
        })
    }

    /// `outputs` に挙げた出力を f32 で返す。
    fn run(
        &self,
        feeds: Vec<(&str, Feed)>,
        outputs: &[&str],
    ) -> Result<HashMap<String, ArrayD<f32>>, PreviewError> {
        let inputs = feeds
            .into_iter()
            .map(|(name, feed)| Ok((name, self.tensor(name, feed)?)))
            .collect::<Result<Vec<(&str, DynTensor)>, PreviewError>>()?;
        let mut session = self
            .session
            .lock()
            .expect("プレビューの ONNX セッションの Mutex が壊れている");
        let result = session.run(inputs).map_err(|e| self.err(e))?;
        outputs
            .iter()
            .map(|name| Ok((name.to_string(), self.output_f32(&result, name)?)))
            .collect()
    }

    fn tensor(&self, name: &str, feed: Feed) -> Result<DynTensor, PreviewError> {
        let ty = self.input_types.get(name).ok_or_else(|| {
            PreviewError::Inference(format!("{}: 入力 {name} がモデルに無い", self.name))
        })?;
        let tensor = match (feed, ty) {
            (Feed::F32(a), TensorElementType::Float32) => Tensor::from_array(a).map(|t| t.upcast()),
            (Feed::F32(a), TensorElementType::Float16) => {
                Tensor::from_array(a.mapv(f16::from_f32)).map(|t| t.upcast())
            }
            (Feed::I32(a), TensorElementType::Int32) => Tensor::from_array(a).map(|t| t.upcast()),
            (Feed::I32(a), TensorElementType::Int64) => {
                Tensor::from_array(a.mapv(i64::from)).map(|t| t.upcast())
            }
            (_, other) => {
                return Err(PreviewError::Inference(format!(
                    "{}: 入力 {name} の型 {other:?} には対応していない",
                    self.name
                )));
            }
        };
        tensor.map_err(|e| self.err(e))
    }

    fn output_f32(
        &self,
        outputs: &SessionOutputs,
        name: &str,
    ) -> Result<ArrayD<f32>, PreviewError> {
        let value = outputs
            .get(name)
            .ok_or_else(|| PreviewError::Inference(format!("{}: 出力 {name} が無い", self.name)))?;
        let (shape, data): (Vec<usize>, Vec<f32>) = match value.dtype().tensor_type() {
            Some(TensorElementType::Float32) => {
                let (shape, data) = value.try_extract_tensor::<f32>().map_err(|e| self.err(e))?;
                (shape.iter().map(|&d| d as usize).collect(), data.to_vec())
            }
            Some(TensorElementType::Float16) => {
                let (shape, data) = value.try_extract_tensor::<f16>().map_err(|e| self.err(e))?;
                (
                    shape.iter().map(|&d| d as usize).collect(),
                    data.iter().map(|v| v.to_f32()).collect(),
                )
            }
            other => {
                return Err(PreviewError::Inference(format!(
                    "{}: 出力 {name} の型 {other:?} には対応していない",
                    self.name
                )));
            }
        };
        ArrayD::from_shape_vec(shape, data).map_err(|e| PreviewError::Inference(e.to_string()))
    }

    fn err(&self, e: ort::Error) -> PreviewError {
        PreviewError::Inference(format!("{}: {e}", self.name))
    }
}

#[cfg(test)]
mod segmenter_tiny_test {
    use super::*;
    use crate::video_cutout::job::point::NormalizedPoint;

    #[test]
    fn pixel_values_are_normalized_chw() {
        // 一様な色なら引き伸ばしても同じ色。期待値は (v/255 - mean) / std を手で計算したもの
        let frame = RgbImage::from_pixel(3, 2, image::Rgb([255, 0, 51]));
        let mean = [0.485, 0.456, 0.406];
        let std = [0.229, 0.224, 0.225];
        let x = to_pixel_values(&frame, mean, std);
        assert_eq!(x.shape(), &[1, 3, 512, 512]);
        let expect = [
            (1.0 - 0.485) / 0.229,
            (0.0 - 0.456) / 0.224,
            (0.2 - 0.406) / 0.225,
        ];
        for c in 0..3 {
            for (y, xx) in [(0, 0), (511, 511), (100, 300)] {
                assert!((x[[0, c, y, xx]] - expect[c]).abs() < 1e-5, "c={c}");
            }
        }
    }

    #[test]
    fn pixel_values_follow_resized_pixels() {
        // 左が黒・右が白の 2x1 → 512 に引き伸ばすと左端は黒、右端は白、真ん中はその間
        let frame = RgbImage::from_raw(2, 1, vec![0, 0, 0, 255, 255, 255]).unwrap();
        let x = to_pixel_values(&frame, [0.0; 3], [1.0; 3]);
        assert_eq!(x[[0, 0, 7, 0]], 0.0);
        assert_eq!(x[[0, 1, 7, 511]], 1.0);
        let mid = x[[0, 2, 7, 256]];
        assert!(mid > 0.0 && mid < 1.0);
    }

    #[test]
    fn point_inputs_scale_to_512_and_label_include_as_one() {
        let points = Points::new(vec![
            NormalizedPoint::new(0.5, 0.25, PointLabel::Include).unwrap(),
            NormalizedPoint::new(1.0, 0.0, PointLabel::Exclude).unwrap(),
        ])
        .unwrap();
        let (coords, labels) = point_inputs(&points);
        assert_eq!(coords.shape(), &[1, 1, 2, 2]);
        assert_eq!(
            coords.iter().copied().collect::<Vec<_>>(),
            vec![256.0, 128.0, 512.0, 0.0]
        );
        assert_eq!(labels.shape(), &[1, 1, 2]);
        assert_eq!(labels.iter().copied().collect::<Vec<_>>(), vec![1, 0]);
    }

    /// 実モデルで動かす（`SAM2_TINY_MODEL_DIR` に `models/sam2.1-tiny-video` と同じ構成のディレクトリを指定）。
    /// 例: `SAM2_TINY_MODEL_DIR=models/sam2.1-tiny-video cargo test -- --ignored tiny_model`
    #[test]
    #[ignore]
    fn tiny_model_encodes_and_decodes() {
        let Ok(dir) = std::env::var("SAM2_TINY_MODEL_DIR") else {
            eprintln!("SAM2_TINY_MODEL_DIR が無いので飛ばす");
            return;
        };
        let segmenter = Sam2TinyPreviewSegmenter::new(Path::new(&dir)).unwrap();
        let frame = RgbImage::from_fn(320, 240, |x, y| {
            image::Rgb([(x % 256) as u8, (y % 256) as u8, 128])
        });
        let features = segmenter.encode(&frame).unwrap();
        assert_eq!(features.feats0.shape(), &[1, 32, 128, 128]);
        assert_eq!(features.feats1.shape(), &[1, 64, 64, 64]);
        assert_eq!(features.feats2_no_mem.shape(), &[1, 256, 32, 32]);
        assert_eq!((features.frame_width, features.frame_height), (320, 240));
        let points = Points::new(vec![
            NormalizedPoint::new(0.5, 0.5, PointLabel::Include).unwrap(),
            NormalizedPoint::new(0.1, 0.1, PointLabel::Exclude).unwrap(),
        ])
        .unwrap();
        let logits = segmenter.decode(&features, &points).unwrap();
        assert_eq!(logits.dim(), (512, 512));
        assert!(logits.iter().all(|v| v.is_finite()));
    }
}
