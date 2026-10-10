use crate::{common::traits::ErrorType::InvalidInput, leaf_error};

leaf_error!(
    pub enum ImageSizeError {
        LongHeight => ("LONG_HEIGHT", InvalidInput),
        LongWidth => ("LONG_WIDTH", InvalidInput),
        ShortHeight => ("SHORT_HEIGHT", InvalidInput),
        ShortWidth => ("SHORT_WIDTH", InvalidInput),
    }
);

leaf_error!(
    pub enum ImageIdError {
        InvalidImageId => ("INVALID_IMAGE_ID", InvalidInput),
    }
);
