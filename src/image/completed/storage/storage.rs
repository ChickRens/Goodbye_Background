use std::sync::Arc;

use crate::image::{
    completed::{completed_image::CompletedImage, storage::error::CompletedImageStorageError},
    image::id::ImageId,
};

pub trait CompletedImageRepository {
    fn save(&self, completed_image: CompletedImage);
    fn get(&self, image_id: ImageId) -> Result<Arc<CompletedImage>, CompletedImageStorageError>;
}
