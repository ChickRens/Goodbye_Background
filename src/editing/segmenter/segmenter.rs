//! 画像セグメンタの抽象トレイト（実装は `Sam2Segmenter` のみ）。
//!
//! 関連型:
//! - `StaticContext`    : 画像ごとに1回だけ計算する重い前処理結果（SAM2 なら画像特徴量）
//! - `InferenceContext` : 推論のたびに更新される状態（SAM2 なら直前のマスク）
//!
//! `segment_fast` と `segment_high_quality` の違いはマスクの後処理だけ（推論は同じ）:
//! - fast : リサイズ → 適用（segment/undo/redo のプレビュー用）
//! - high_quality : リサイズ → ガイデッドフィルタで平滑化 → 適用（save 用）
//!
//! `input_points` は **元画像座標**。1024 座標への変換は実装側で行う。
//! `input_image` は現状の SAM2 実装では使われていない（特徴量は StaticContext にあるため）。

use crate::{
    editing::segmenter::error::{SegmenterLoadingError, SegmenterRuntimeError},
    editing::segmenter::segmented_image::SegmentedImage,
    image::{
        original::original_image::OriginalImage,
        segmenter_input::segmenter_input_image::SegmenterInputImage,
    },
    point::point::Point,
};

pub trait ImageSegmenter: Send + Sync {
    type StaticContext;
    type InferenceContext;

    fn prepare_inference_context(
        &self,
        image: &SegmenterInputImage,
    ) -> Result<Self::InferenceContext, SegmenterLoadingError>;
    fn prepare_static_context(
        &self,
        image: &SegmenterInputImage,
    ) -> Result<Self::StaticContext, SegmenterLoadingError>;
    fn segment_fast(
        &self,
        input_image: &SegmenterInputImage,
        original_image: &OriginalImage,
        static_context: &Self::StaticContext,
        inference_context: &Self::InferenceContext,
        input_points: &[Point],
    ) -> Result<(Self::InferenceContext, SegmentedImage), SegmenterRuntimeError>;
    fn segment_high_quality(
        &self,
        input_image: &SegmenterInputImage,
        original_image: &OriginalImage,
        static_context: &Self::StaticContext,
        inference_context: &Self::InferenceContext,
        input_points: &[Point],
    ) -> Result<(Self::InferenceContext, SegmentedImage), SegmenterRuntimeError>;
}
