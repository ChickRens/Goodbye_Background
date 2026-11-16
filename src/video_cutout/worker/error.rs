//! ワーカー投入のエラー。`StartVideoJob` 内で `Failed(DispatchFailed)` に変換するので HTTP には出ない。

use crate::{common::traits::ErrorType, leaf_detail_error};

leaf_detail_error!(
    pub enum WorkerError {
        Dispatch => ("WORKER_DISPATCH_FAILED", ErrorType::Internal),
    }
);
