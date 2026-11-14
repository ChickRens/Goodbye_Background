//! SAM2 への入力画像（1024x1024 の生 RGBA）。
//!
//! 現状、保存はされるが `Sam2Segmenter::segment_*` では使われていない（引数で受け取るだけ）。
//! 画像エンコードは `/upload` 時に1回だけ行い、結果は `SAM2StaticContext` に保持している。

pub mod generator;
pub mod segmenter_input_image;
pub mod storage;
