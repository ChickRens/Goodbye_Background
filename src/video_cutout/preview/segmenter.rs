//! プレビューのセグメンタ（trait）。実装は `segmenter_sam2.rs`（SAM2.1 small。本番のワーカーと同じモデル）。テストでは偽物を差し込む。

use half::f16;
use image::RgbImage;
use ndarray::{Array2, Array4};

use crate::video_cutout::{job::point::Points, preview::error::PreviewError};

/// `constants.json` に `image_size` が無いときのモデルの入力の一辺（tiny は 512、small 以上は 1024。
/// 実際の値は `constants.json` から読む。`worker/cutout/tracker.py` の `DEFAULT_IMAGE_SIZE` と同じ）。
pub const DEFAULT_PREVIEW_INPUT_SIZE: usize = 512;

/// 1 フレームぶんのエンコード結果と、登録したフレームの大きさ。
/// 特徴量は f16 で持つ（1 ジョブ: small で約 8MB、tiny で約 2MB）。モデルが fp16 なので、f32 で持つのと結果は変わらない。
pub struct PreviewFeatures {
    /// `[1,32,S/4,S/4]`（S = モデルの入力の一辺。small は 1024）
    pub feats0: Array4<f16>,
    /// `[1,64,S/8,S/8]`
    pub feats1: Array4<f16>,
    /// `[1,256,S/16,S/16]`。1 枚目はメモリ注意を通さないので、これをそのまま `feats2_cond` にする
    pub feats2_no_mem: Array4<f16>,
    /// 返すマスクの PNG の大きさ（登録したフレームと同じ）
    pub frame_width: u32,
    pub frame_height: u32,
}

/// デコードの結果。
pub struct PreviewDecoded {
    /// `[S, S]` のロジット（まだ sigmoid していない）。
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

    /// 点からマスク（`[S, S]` のロジット）と対象のスコアを出す。
    fn decode(
        &self,
        features: &PreviewFeatures,
        points: &Points,
    ) -> Result<PreviewDecoded, PreviewError>;
}
