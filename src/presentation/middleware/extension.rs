//! エラーハンドラ → ミドルウェアへ情報を渡すためのレスポンス拡張(extensions)。
//!
//! `AppError::into_response` が失敗時にエラーコードを詰め、
//! `usage_tracking::track_usage` がそれを読んで「失敗」として DB に記録する。
//! この拡張が無いレスポンス = 成功、という判定になっている。

#[derive(Clone)]
pub struct UsageStatusExt {
    pub code: String,
}
