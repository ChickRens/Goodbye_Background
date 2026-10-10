use crate::{common::traits::ErrorType::NotFound, leaf_error};

leaf_error!(
    pub enum SessionRepositoryError {
        SessionNotFound => ("SESSION_NOT_FOUND", NotFound),
    }
);
