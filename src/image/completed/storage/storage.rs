use std::ops::DerefMut;

use crate::{
    application::{
        interface::completed_image_repository::error::CompletedImageRepositoryError,
        types::completed_image::CompletedImage,
    },
    domain::value_object::image_id::image_id::ImageId,
};

pub trait CompletedImageRepository {
    type Guard<'a>: DerefMut<Target = CompletedImage>
    where
        Self: 'a;

    fn save(&self, completed_image: CompletedImage);
    fn get<'a>(
        &'a self,
        image_id: ImageId,
    ) -> Result<Self::Guard<'a>, CompletedImageRepositoryError>;
}
