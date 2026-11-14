//! ハンドラが返す最上位のエラー型 `AppError` と、その HTTP レスポンスへの変換。
//!
//! `AppError` = ユースケース由来（`ApplicationError`）+ HTTP 層由来（`PresentationError`、動画のハンドラの `VideoHandlerError`）。
//! どちらも `From` があるので、ハンドラ内では `?` だけで変換できる。

use axum::{
    Json,
    http::StatusCode,
    response::{IntoResponse, Response},
};

use crate::{
    api::handler::video_cutout::error::VideoHandlerError,
    application::error::ApplicationError,
    common::traits::{Cause, Code, ErrorType, ErrorTypeProvider},
    parent_error,
    presentation::{
        errors::presentation_error::PresentationError, middleware::extension::UsageStatusExt,
    },
};

parent_error!(
    pub enum AppError {
        ApplicationError(ApplicationError),
        Presentation(PresentationError),
        VideoHandler(VideoHandlerError),
    }
);

/// エラー時のレスポンスボディ。`{"code": "SESSION_NOT_FOUND"}` のような形。
/// 詳細メッセージ（`cause`）はクライアントには返さず、サーバーログにだけ出す。
#[derive(serde::Serialize)]
struct ErrorBody {
    code: String,
}

impl IntoResponse for AppError {
    /// エラー → HTTP レスポンス変換。全ハンドラのエラーは最終的にここを通る。
    ///
    /// 1. `error_type()` で HTTP ステータスを決める
    /// 2. `code()` を JSON ボディに入れる
    /// 3. 同じ code を `UsageStatusExt` としてレスポンス拡張に入れる
    ///    → `track_usage` ミドルウェアが「失敗」として記録するための目印
    fn into_response(self) -> Response {
        let status = match self.error_type() {
            ErrorType::NotFound => StatusCode::NOT_FOUND,
            ErrorType::Conflict => StatusCode::CONFLICT,
            ErrorType::Internal => StatusCode::INTERNAL_SERVER_ERROR,
            ErrorType::InvalidInput => StatusCode::BAD_REQUEST,
            ErrorType::Unauthorized => StatusCode::UNAUTHORIZED,
            ErrorType::TooManyRequests => StatusCode::TOO_MANY_REQUESTS,
            ErrorType::Unavailable => StatusCode::SERVICE_UNAVAILABLE,
        };

        let code = self.code();
        let ext = UsageStatusExt {
            code: code.to_string(),
        };

        eprintln!("AppError: {:?}", self.cause());

        let json = Json(ErrorBody {
            code: code.to_string(),
        });

        let mut response = (status, json).into_response();
        response.extensions_mut().insert(ext);
        response
    }
}
