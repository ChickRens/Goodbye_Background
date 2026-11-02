//! 画像セグメンテーション（SAM2）の本体と前後処理。
//!
//! 処理パイプライン（`segmenter_sam2.rs`）:
//! 1. `/upload` 時: 1024x1024 画像 → image encoder → `SAM2StaticContext`（特徴量。セッション中不変）
//! 2. `/segment` 等: ポイント座標(元画像座標) → 1024 座標に変換 → prompt encoder → mask decoder
//!    → 256x256 マスク（ロジット）
//! 3. `mask_resizer`: 256x256 → 元画像サイズへバイリニア補間（ロジットのまま）
//! 4. `mask_activation`: sigmoid でロジット → 0〜1 の確率
//! 5. （save 時のみ）`mask_smoother`: ガイデッドフィルタでエッジを元画像に沿わせる
//! 6. `mask_applier`: 確率 × 255 を元画像のアルファチャンネルに書き込む

pub mod error;
pub mod mask_activation;
pub mod mask_applier;
pub mod mask_resizer;
pub mod mask_smoother;
pub mod prepare_service;
pub mod sam2_data;
pub mod segment_service;
pub mod segmented_image;
pub mod segmenter;
pub mod segmenter_sam2;
