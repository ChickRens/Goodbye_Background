//! 回数制限のエラー。

use crate::{common::traits::ErrorType, leaf_detail_error};

leaf_detail_error!(
    pub enum QuotaError {
        Exceeded => ("QUOTA_EXCEEDED", ErrorType::TooManyRequests),
        Database => ("QUOTA_DB_ERROR", ErrorType::Internal),
    }
);
