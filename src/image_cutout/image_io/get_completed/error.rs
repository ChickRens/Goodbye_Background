//! 完成画像取得ユースケースのエラー。

use crate::{image_cutout::image::completed::storage::error::CompletedImageStorageError, parent_error};

parent_error!(
    pub enum GetCompletedUseCaseError {
        CompletedService(CompletedImageStorageError),
    }
);
