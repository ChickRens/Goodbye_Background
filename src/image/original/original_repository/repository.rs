use std::sync::Arc;

use crate::image::{
    image::id::ImageId,
    original::{
        original_image::OriginalImage, original_repository::error::OriginalImageRepositoryError,
    },
};

pub trait OriginalImageRepository: Send + Sync {
    fn save(&self, image: OriginalImage);
    fn get(&self, image_id: &ImageId) -> Result<Arc<OriginalImage>, OriginalImageRepositoryError>;
}
