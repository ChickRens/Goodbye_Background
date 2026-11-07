//! プレビューのセグメンタ（trait）。実装は `segmenter_tiny.rs`（SAM2.1 tiny）。テストでは偽物を差し込む。

use half::f16;
use image::RgbImage;
use ndarray::{Array2, Array4};

use crate::video_cutout::{job::point::Points, preview::error::PreviewError};

/// モデルの入力の一辺（tiny は 512x512 に引き伸ばして入れる）。
pub const PREVIEW_INPUT_SIZE: usize = 512;

/// 1 フレームぶんのエンコード結果と、登録したフレームの大きさ。
/// 特徴量は f16 で持つ（1 ジョブ約 2MB）。モデルが fp16 なので、f32 で持つのと結果は変わらない。
pub struct PreviewFeatures {
    /// `[1,32,128,128]`
    pub feats0: Array4<f16>,
    /// `[1,64,64,64]`
    pub feats1: Array4<f16>,
    /// `[1,256,32,32]`。1 枚目はメモリ注意を通さないので、これをそのまま `feats2_cond` にする
    pub feats2_no_mem: Array4<f16>,
    /// 返すマスクの PNG の大きさ（登録したフレームと同じ）
    pub frame_width: u32,
    pub frame_height: u32,
}

/// デコードの結果。
pub struct PreviewDecoded {
    /// `[512, 512]` のロジット（まだ sigmoid していない）。
    pub logits: Array2<f32>,
    /// `object_score_logits`。0 より大きければ対象が写っている（ワーカーの `subject_not_found` と同じ判定）。
    pub object_score: f32,
}

impl PreviewDecoded {
    pub fn object_present(&self) -> bool {
        self.object_score > 0.0
    }
}

pub trait PreviewSegmenter: Send + Sync {
    /// フレームを 1 回だけエンコードする（重い。CPU で数百 ms〜）。
    fn encode(&self, frame: &RgbImage) -> Result<PreviewFeatures, PreviewError>;

    /// 点からマスク（`[512, 512]` のロジット）と対象のスコアを出す。
    fn decode(
        &self,
        features: &PreviewFeatures,
        points: &Points,
    ) -> Result<PreviewDecoded, PreviewError>;
}
