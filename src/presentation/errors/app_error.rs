use axum::{
    Json,
    http::StatusCode,
    response::{IntoResponse, Response},
};

use crate::{
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
    }
);

#[derive(serde::Serialize)]
struct ErrorBody {
    code: String,
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let status = match self.error_type() {
            ErrorType::NotFound => StatusCode::NOT_FOUND,
            ErrorType::Conflict => StatusCode::CONFLICT,
            ErrorType::Internal => StatusCode::INTERNAL_SERVER_ERROR,
            ErrorType::InvalidInput => StatusCode::BAD_REQUEST,
            ErrorType::Unauthorized => StatusCode::UNAUTHORIZED,
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
