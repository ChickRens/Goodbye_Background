use std::sync::Arc;

use crate::image::{
    image::id::ImageId,
    segmenter_input::{
        segmenter_input_image::SegmenterInputImage, storage::error::SegmenterInputImageStorageError,
    },
};

pub trait SegmenterInputImageStorage: Send + Sync {
    fn save(&self, image: SegmenterInputImage);
    fn get(
        &self,
        image_id: ImageId,
    ) -> Result<Arc<SegmenterInputImage>, SegmenterInputImageStorageError>;
}