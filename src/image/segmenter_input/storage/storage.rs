use std::ops::DerefMut;

use crate::{
    application::{
        interface::segmenter_input_image_storage::error::SegmenterInputImageStorageError,
        types::segmenter_input_image::SegmenterInputImage,
    },
    domain::value_object::image_id::image_id::ImageId,
};

pub trait SegmenterInputImageStorage {
    type Guard<'a>: DerefMut<Target = SegmenterInputImage>
    where
        Self: 'a;

    fn save(&self, image: SegmenterInputImage);
    fn get<'a>(
        &'a self,
        image_id: ImageId,
    ) -> Result<Self::Guard<'a>, SegmenterInputImageStorageError>;
}
