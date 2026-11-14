//! セッション取得時のエラー。期限切れ(GC済み)の場合もここに来る。

use crate::{common::traits::ErrorType::NotFound, leaf_error};

leaf_error!(
    pub enum SessionRepositoryError {
        SessionNotFound => ("SESSION_NOT_FOUND", NotFound),
    }
);
