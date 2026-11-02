//! 画像セグメンタの抽象トレイト（実装は `Sam2Segmenter` のみ）。
//!
//! 関連型:
//! - `StaticContext`    : 画像ごとに1回だけ計算する重い前処理結果（SAM2 なら画像特徴量）
//! - `InferenceContext` : 推論のたびに更新される状態（SAM2 なら直前のマスク）
//!
//! `segment_fast` と `segment_high_quality` の違いはマスクの後処理と出力解像度（推論は同じ）:
//! - fast : 任意解像度（通常プレビュー解像度）にリサイズ → 適用（segment/undo/redo のプレビュー用）
//! - high_quality : 元画像サイズにリサイズ → ガイデッドフィルタで平滑化 → 適用（save 用）
//!
//! `input_points` は **元画像座標**。1024 座標への変換は実装側で行う。
//! `input_image` は現状の SAM2 実装では使われていない（特徴量は StaticContext にあるため）。

use crate::{
    editing::segmenter::error::{SegmenterLoadingError, SegmenterRuntimeError},
    editing::segmenter::segmented_image::SegmentedImage,
    image::{
        image::image::Image,
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
    /// 推論し、`output_image` を切り抜いて返す。
    ///
    /// `original_image` は座標変換（元画像座標 → 1024 座標）のためのサイズ情報として使う。
    /// `output_image` は切り抜く対象で解像度は任意（通常はプレビュー解像度に縮小した元画像）。
    /// マスクは `output_image` のサイズに合わせてリサイズされる。
    fn segment_fast(
        &self,
        input_image: &SegmenterInputImage,
        original_image: &OriginalImage,
        static_context: &Self::StaticContext,
        inference_context: &Self::InferenceContext,
        input_points: &[Point],
        output_image: &Image,
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
