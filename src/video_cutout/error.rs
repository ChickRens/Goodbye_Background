//! 動画切り抜き機能全体のエラー（ユースケースに入る前に決まるもの）。

use crate::{common::traits::ErrorType, leaf_error};

leaf_error!(
    pub enum VideoCutoutError {
        Disabled => ("VIDEO_CUTOUT_DISABLED", ErrorType::Unavailable),
        // プレビュー用フレームの登録（エンコード）が同時に上限まで動いている
        PreviewBusy => ("PREVIEW_BUSY", ErrorType::Unavailable),
    }
);
