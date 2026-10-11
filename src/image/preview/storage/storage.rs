use std::ops::DerefMut;

use crate::image::{
    image::id::ImageId,
    preview::{preview::PreviewImage, storage::error::PreviewStorageError},
};

pub trait PreviewStorage {
    type Guard<'a>: DerefMut<Target = PreviewImage>
    where
        Self: 'a;

    fn save_as_original(&self, original: PreviewImage);
    fn save_as_segmented(&self, segmented: PreviewImage);
    fn get<'a>(&'a self, image_id: ImageId) -> Result<Self::Guard<'a>, PreviewStorageError>;
}
