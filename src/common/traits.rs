//! エラー型が共通で実装するトレイト群。
//!
//! 全てのエラー enum はこの3つを実装し、最終的に `presentation::errors::app_error::AppError`
//! で HTTP レスポンスに変換される。
//!
//! - `Code`  : クライアントに返す機械可読なエラーコード（例: `"SESSION_NOT_FOUND"`）。フロントはこの文字列で表示を分岐する
//! - `Cause` : ログ用の詳細メッセージ（無ければ `None`）。クライアントには返さない
//! - `ErrorTypeProvider` : HTTP ステータスの分類（`ErrorType` → 400/401/404/409/429/500/503）

pub trait Code {
    fn code(&self) -> &str;
}

pub trait Cause {
    fn cause(&self) -> Option<&str>;
}

/// エラーの分類。`AppError::into_response` で HTTP ステータスに変換される。
pub enum ErrorType {
    /// 400 Bad Request: クライアントの入力が不正（ID 形式、画像サイズ、undo できない等）
    InvalidInput,
    /// 409 Conflict: 状態の矛盾（現状はプレビュー ID の重複のみ）
    Conflict,
    /// 404 Not Found: セッションや画像が見つからない（TTL 切れで GC された場合も含む）
    NotFound,
    /// 500 Internal Server Error: 推論失敗などサーバー側の問題
    Internal,
    /// 401 Unauthorized: 認証情報（クライアント ID など）が無い・不正
    Unauthorized,
    /// 429 Too Many Requests: 利用上限（クォータ）超過
    TooManyRequests,
    /// 503 Service Unavailable: 外部サービスが一時的に使えない
    Unavailable,
}

pub trait ErrorTypeProvider {
    fn error_type(&self) -> ErrorType;
}
