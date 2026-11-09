//! プレビューのエラー。

use crate::{common::traits::ErrorType, leaf_detail_error, leaf_error};

leaf_error!(
    pub enum PreviewInputError {
        InvalidImage => ("INVALID_PREVIEW_IMAGE", ErrorType::InvalidInput),
        FrameNotFound => ("PREVIEW_FRAME_NOT_FOUND", ErrorType::Conflict),
        // 1 ジョブで点を打てるフレーム（MAX_PROMPTS = 5 枚）を超えた
        TooManyFrames => ("TOO_MANY_PREVIEW_FRAMES", ErrorType::Conflict),
        // フレームの時刻が無い・整数でない・範囲の外
        InvalidTime => ("INVALID_PREVIEW_TIME", ErrorType::InvalidInput),
    }
);

// モデルの読み込み・推論・PNG の書き出しの失敗。詳細はログにだけ出す
leaf_detail_error!(
    pub enum PreviewError {
        ModelLoad => ("PREVIEW_FAILED", ErrorType::Internal),
        Inference => ("PREVIEW_FAILED", ErrorType::Internal),
    }
);
