//! HTTP 層（axum）の横断的な部品: ルーティング・ミドルウェア・エラー→レスポンス変換。
//! 個々のエンドポイント処理は `crate::api::handler` にある。

pub mod errors;
pub mod middleware;
pub mod router;
