//! 編集ユースケース群（SAM2 による切り抜き・undo/redo・保存）と、その中核ロジック。
//!
//! - `prepare_segment` : アップロード直後に SAM2 で画像をエンコードし編集セッションを作る
//! - `segment` / `undo` / `redo` : ポイント操作 → 推論 → プレビュー生成
//! - `save` : 高品質版（エッジ平滑化あり）で再推論 → 保存用 WebP 生成
//! - `segmenter` : SAM2 推論本体と、その前後処理（マスクのリサイズ・平滑化・適用）
//! - `session` : 編集セッション（SAM2 特徴量 + ポイント履歴 + マスク履歴）

pub mod prepare_segment;
pub mod redo;
pub mod save;
pub mod segment;
pub mod segmenter;
pub mod session;
pub mod undo;
