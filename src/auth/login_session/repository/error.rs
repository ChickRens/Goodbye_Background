use crate::{common::traits::ErrorType::Internal, leaf_detail_error};

leaf_detail_error!(
    pub enum LoginSessionRepositoryError {
        Database => ("LOGIN_SESSION_REPOSITORY_FAILED", Internal),
    }
);
