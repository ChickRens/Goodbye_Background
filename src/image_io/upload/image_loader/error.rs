use crate::{
    common::traits::{Cause, Code, ErrorType, ErrorTypeProvider},
    domain::value_object::image_size::error::ImageSizeError,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LoadingError {
    ImageSize(ImageSizeError),
    UnsupportedFormat(String),
    CorruptedImage(String),
}

impl Code for LoadingError {
    fn code(&self) -> &str {
        match self {
            Self::ImageSize(err) => err.code(),
            Self::UnsupportedFormat(_) => "UNSUPPORTED_FORMAT",
            Self::CorruptedImage(_) => "CORRUPTED_IMAGE",
        }
    }
}

impl Cause for LoadingError {
    fn cause(&self) -> Option<&str> {
        match self {
            Self::ImageSize(err) => err.cause(),
            Self::UnsupportedFormat(err) => Some(err),
            Self::CorruptedImage(err) => Some(err),
        }
    }
}

impl ErrorTypeProvider for LoadingError {
    fn error_type(&self) -> ErrorType {
        match self {
            Self::ImageSize(err) => err.error_type(),
            Self::CorruptedImage(_) => ErrorType::InvalidInput,
            Self::UnsupportedFormat(_) => ErrorType::InvalidInput,
        }
    }
}

impl From<ImageSizeError> for LoadingError {
    fn from(value: ImageSizeError) -> Self {
        LoadingError::ImageSize(value)
    }
}
