use crate::{
    common::traits::ErrorType::{Conflict, NotFound},
    leaf_error,
};

leaf_error!(
    pub enum PreviewStorageError {
        ImageNotFound => ("IMAGE_NOT_FOUND", NotFound),
        AmbiguousId => ("AMBIGUOUS_ID", Conflict),
    }
);
