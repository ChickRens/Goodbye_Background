//! 動画切り抜き機能全体のエラー（ユースケースに入る前に決まるもの）。

use crate::{common::traits::ErrorType, leaf_error};

leaf_error!(
    pub enum VideoCutoutError {
        Disabled => ("VIDEO_CUTOUT_DISABLED", ErrorType::Unavailable),
    }
);
