//! 保存用画像の生成。
//!
//! 注意: preview / segmenter_input とは逆で、ここは `generator.rs` が実装（WebP）、
//! `generator_impl.rs` がトレイトになっている（命名の不統一）。

pub mod generator;
pub mod generator_impl;
