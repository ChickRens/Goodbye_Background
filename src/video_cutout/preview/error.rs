//! プレビューのエラー。

use crate::{common::traits::ErrorType, leaf_detail_error, leaf_error};

leaf_error!(
    pub enum PreviewInputError {
        InvalidImage => ("INVALID_PREVIEW_IMAGE", ErrorType::InvalidInput),
        FrameNotFound => ("PREVIEW_FRAME_NOT_FOUND", ErrorType::Conflict),
    }
);

// モデルの読み込み・推論・PNG の書き出しの失敗。詳細はログにだけ出す
leaf_detail_error!(
    pub enum PreviewError {
        ModelLoad => ("PREVIEW_FAILED", ErrorType::Internal),
        Inference => ("PREVIEW_FAILED", ErrorType::Internal),
    }
);
