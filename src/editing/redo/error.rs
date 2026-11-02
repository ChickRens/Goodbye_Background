//! redo ユースケースのエラー。
//!
//! 注意: `SegmenterInputStorage` variant は現状どこからも生成されない（SegmentServiceError 側に含まれるため）。

use crate::{
    editing::segmenter::error::SegmentServiceError,
    image::segmenter_input::storage::error::SegmenterInputImageStorageError, parent_error,
};

parent_error!(
    pub enum RedoUseCaseError {
        SegmentService(SegmentServiceError),
        SegmenterInputStorage(SegmenterInputImageStorageError),
    }
);
