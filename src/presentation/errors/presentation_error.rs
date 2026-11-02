//! HTTP 層（リクエストのパース等）で起きるエラー。
//!
//! 他のエラーと違いマクロを使わず手書きで `Code`/`Cause`/`ErrorTypeProvider` を実装している
//! （詳細メッセージ付きの variant と子エラーの variant が混在しているため）。
//!
//! - `Multipart`   : multipart の読み取り失敗（500 扱い）
//! - `UnknownName` : `/upload` で `file` 以外のフィールド名が来た
//! - `NoName`      : multipart フィールドに名前が無い
//! - `SessionId` / `ImageId` : リクエストの ID が UUID として不正
//! - `BlockingTask`: `spawn_blocking` のタスクが panic した（推論中の `expect` 失敗など）

use crate::{
    common::traits::{
        Cause, Code,
        ErrorType::{self, Internal, InvalidInput},
        ErrorTypeProvider,
    },
    image::image::error::ImageIdError,
    session::error::SessionIdError,
};

#[derive(Debug, PartialEq, Eq, Clone)]
pub enum PresentationError {
    Multipart(String),
    UnknownName(String),
    SessionId(SessionIdError),
    ImageId(ImageIdError),
    BlockingTask(String),
    NoName,
}

impl Code for PresentationError {
    fn code(&self) -> &str {
        match self {
            Self::Multipart(_) => "MULTIPART_ERROR",
            Self::UnknownName(_) => "UNKNOWN_TAG_NAME",
            Self::ImageId(err) => err.code(),
            Self::SessionId(err) => err.code(),
            Self::BlockingTask(_) => "BLOCKING_TASK_FAILED",
            Self::NoName => "NO_TAG_NAME_ERROR",
        }
    }
}

impl Cause for PresentationError {
    fn cause(&self) -> Option<&str> {
        match self {
            Self::Multipart(cause) => Some(cause),
            Self::UnknownName(cause) => Some(cause),
            Self::ImageId(err) => err.cause(),
            Self::SessionId(err) => err.cause(),
            Self::BlockingTask(cause) => Some(cause),
            Self::NoName => None,
        }
    }
}

impl From<ImageIdError> for PresentationError {
    fn from(value: ImageIdError) -> Self {
        Self::ImageId(value)
    }
}

impl From<SessionIdError> for PresentationError {
    fn from(value: SessionIdError) -> Self {
        Self::SessionId(value)
    }
}

impl ErrorTypeProvider for PresentationError {
    fn error_type(&self) -> ErrorType {
        match self {
            Self::Multipart(_) => Internal,
            Self::UnknownName(_) => InvalidInput,
            Self::ImageId(err) => err.error_type(),
            Self::SessionId(err) => err.error_type(),
            Self::BlockingTask(_) => Internal,
            Self::NoName => InvalidInput,
        }
    }
}
