//! アップロード直後の SAM2 前処理ユースケース（HTTP エンドポイントは無い。`App::upload` から呼ばれる）。

pub mod error;
pub mod input;
pub mod usecase;
