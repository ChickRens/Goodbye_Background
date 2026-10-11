use std::ops::DerefMut;

use crate::image::{
    completed::{completed_image::CompletedImage, storage::error::CompletedImageStorageError},
    image::id::ImageId,
};

pub trait CompletedImageRepository {
    type Guard<'a>: DerefMut<Target = CompletedImage>
    where
        Self: 'a;

    fn save(&self, completed_image: CompletedImage);
    fn get<'a>(&'a self, image_id: ImageId) -> Result<Self::Guard<'a>, CompletedImageStorageError>;
}
