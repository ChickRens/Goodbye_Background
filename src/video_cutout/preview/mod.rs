//! 動画のプレビュー（1 フレーム目の切り抜き範囲の確認）。
//!
//! 本番の GPU ワーカー（`worker/cutout/tracker.py` の 1 枚目）と同じ SAM2.1 tiny（512px）を、
//! Cloud Run の CPU で ort を使って動かす。フレームの登録時に 1 回だけ `vision_encoder` を回して特徴量をジョブごとに置き、
//! 点が変わるたびに `mask_decoder` だけを回す（「プレビューで見えた範囲 = 出力の 1 フレーム目」にするため）。

pub mod error;
pub mod feature_store;
pub mod image_math;
pub mod segmenter;
pub mod segmenter_tiny;
