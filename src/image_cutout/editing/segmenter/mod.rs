//! 画像セグメンテーション（SAM2）の本体と前後処理。
//!
//! 処理パイプライン（`segmenter_sam2.rs`）:
//! 1. `/upload` 時: 1024x1024 画像 → image encoder → `SAM2StaticContext`（特徴量。セッション中不変）
//! 2. `/segment` 等: ポイント座標(元画像座標) → 1024 座標に変換 → prompt encoder → mask decoder
//!    → 256x256 マスク（ロジット）
//! 3. `mask_resizer`: 256x256 → 出力サイズへバイリニア補間（ロジットのまま）。
//!    出力サイズは segment/undo/redo ならプレビュー解像度、save なら元画像サイズ
//! 4. （save 時のみ）`mask_smoother`: 境界付近だけガイデッドフィルタでエッジを元画像に沿わせる
//! 5. `mask_applier`: clamp(ロジット × 255) を元画像のアルファチャンネルに書き込む
//!    （= ロジット 0 付近の狭い帯以外は完全に透明 / 不透明。sigmoid は掛けないこと）

pub mod error;
pub mod mask_applier;
pub mod mask_resizer;
pub mod mask_smoother;
pub mod prepare_service;
pub mod sam2_data;
pub mod segment_service;
pub mod segmented_image;
pub mod segmenter;
pub mod segmenter_sam2;
