use crate::{
    common::traits::ErrorType::{Internal, InvalidInput},
    leaf_error,
};

leaf_error!(
    pub enum EditingSessionError {
        UndoFailed => ("UNDO_FAILED", InvalidInput),
        RedoFailed => ("REDO_FAILED", InvalidInput),
        HistoryCorrupted => ("HISTORY_CORRUPTED", Internal),
    }
);
