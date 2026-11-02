//! セグメンタ関連のエラー定義。
//!
//! - `SegmenterModelError`  : 起動時の ONNX モデル読み込み失敗
//! - `SegmenterRuntimeError`: 推論失敗（segment/undo/redo/save）
//! - `SegmenterLoadingError`: 前処理（画像エンコード）失敗（upload）
//! - `SegmentServiceError` / `PrepareServiceError`: サービス層でまとめたもの

use crate::{
    common::traits::ErrorType::Internal,
    editing::session::{
        error::EditingSessionError, repository::error::EditingSessionRepositoryError,
    },
    image::{
        original::original_repository::error::OriginalImageRepositoryError,
        segmenter_input::storage::error::SegmenterInputImageStorageError,
    },
    leaf_detail_error, parent_error,
    session::repository::error::SessionRepositoryError,
};

leaf_detail_error!(
    pub enum SegmenterModelError {
        ModelLoadError => ("MODEL_LOAD_ERROR", Internal),
    }
);

leaf_detail_error!(
    pub enum SegmenterRuntimeError {
        InferenceError => ("INFERENCE_ERROR", Internal),
    }
);

leaf_detail_error!(
    pub enum SegmenterLoadingError {
        PreProcessError => ("PREPROCESS_ERROR", Internal),
    }
);

parent_error!(
    pub enum SegmentServiceError {
        SessionRepository(SessionRepositoryError),
        ImageRepository(OriginalImageRepositoryError),
        SegmenterInputStorage(SegmenterInputImageStorageError),
        EditingSessionRepository(EditingSessionRepositoryError),
        Segmenter(SegmenterRuntimeError),
        EditingSession(EditingSessionError),
    }
);

parent_error!(
    pub enum PrepareServiceError {
        Segmenter(SegmenterLoadingError),
        ImageRepository(OriginalImageRepositoryError),
        EditingSessionRepository(EditingSessionRepositoryError),
        SegmenterInputStorage(SegmenterInputImageStorageError),
    }
);
