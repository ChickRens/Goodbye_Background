use std::ops::DerefMut;

use crate::image::{
    image::id::ImageId,
    segmenter_input::{
        segmenter_input_image::SegmenterInputImage, storage::error::SegmenterInputImageStorageError,
    },
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
