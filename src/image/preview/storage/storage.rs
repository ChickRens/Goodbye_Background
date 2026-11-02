//! プレビュー保存先のトレイト。保存は種類別、取得は ID だけで行う。

use std::sync::Arc;

use crate::image::{
    image::id::ImageId,
    preview::{preview::PreviewImage, storage::error::PreviewStorageError},
};

pub trait PreviewStorage: Send + Sync {
    fn save_as_original(&self, original: PreviewImage);
    fn save_as_segmented(&self, segmented: PreviewImage);
    fn get(&self, image_id: ImageId) -> Result<Arc<PreviewImage>, PreviewStorageError>;
}