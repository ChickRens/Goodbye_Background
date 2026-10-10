use std::ops::DerefMut;

use crate::image::{id::ImageId, original_image::OriginalImage, original_repository::error::OriginalImageRepositoryError};

pub trait OriginalImageRepository {
    type Guard<'a>: DerefMut<Target = OriginalImage>
    where
        Self: 'a;

    fn save(&self, image: OriginalImage);
    fn get<'a>(
        &'a self,
        image_id: &ImageId,
    ) -> Result<Self::Guard<'a>, OriginalImageRepositoryError>;
}
