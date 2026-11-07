//! プレビューのセグメンタ（trait）。実装は `segmenter_tiny.rs`（SAM2.1 tiny）。テストでは偽物を差し込む。

use image::RgbImage;
use ndarray::{Array2, Array4};

use crate::video_cutout::{job::point::Points, preview::error::PreviewError};

/// モデルの入力の一辺（tiny は 512x512 に引き伸ばして入れる）。
pub const PREVIEW_INPUT_SIZE: usize = 512;

/// 1 フレームぶんのエンコード結果（f32 で持つ。1 ジョブ約 4MB）と、登録したフレームの大きさ。
pub struct PreviewFeatures {
    /// `[1,32,128,128]`
    pub feats0: Array4<f32>,
    /// `[1,64,64,64]`
    pub feats1: Array4<f32>,
    /// `[1,256,32,32]`。1 枚目はメモリ注意を通さないので、これをそのまま `feats2_cond` にする
    pub feats2_no_mem: Array4<f32>,
    /// 返すマスクの PNG の大きさ（登録したフレームと同じ）
    pub frame_width: u32,
    pub frame_height: u32,
}

pub trait PreviewSegmenter: Send + Sync {
    /// フレームを 1 回だけエンコードする（重い。CPU で数百 ms〜）。
    fn encode(&self, frame: &RgbImage) -> Result<PreviewFeatures, PreviewError>;

    /// 点からマスクを出す。戻り値は `[512, 512]` のロジット（まだ sigmoid していない）。
    fn decode(
        &self,
        features: &PreviewFeatures,
        points: &Points,
    ) -> Result<Array2<f32>, PreviewError>;
}
