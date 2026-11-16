//! オブジェクトストレージのエラー。

use crate::{common::traits::ErrorType, leaf_detail_error};

leaf_detail_error!(
    pub enum StorageError {
        Request => ("STORAGE_FAILED", ErrorType::Internal),
    }
);
