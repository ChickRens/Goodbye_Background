use std::sync::Arc;

use crate::image::{
    image::id::ImageId,
    preview::{preview::PreviewImage, storage::error::PreviewStorageError},
};

pub trait PreviewStorage {
    fn save_as_original(&self, original: PreviewImage);
    fn save_as_segmented(&self, segmented: PreviewImage);
    fn get(&self, image_id: ImageId) -> Result<Arc<PreviewImage>, PreviewStorageError>;
}
