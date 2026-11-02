//! プレビューの生成と保存をまとめたサービス。
//!
//! - `generate_and_save_original`  : `/upload` 時。元画像のプレビュー。戻り値は座標変換用の倍率
//! - `generate_and_save_segmented` : segment/undo/redo 時。切り抜き結果のプレビュー（倍率は捨てる）
//! - `get` : `/get-preview` 用。Arc から clone して返す（WebP なので小さい）

use std::{sync::Arc, time::Instant};

use crate::image::{
    image::{id::ImageId, image::Image},
    preview::{
        generator::generator::PreviewImageGenerator,
        preview::PreviewImage,
        storage::{error::PreviewStorageError, storage::PreviewStorage},
    },
};

pub trait PreviewService: Send + Sync {
    fn generate_and_save_original(&self, image: &Image) -> f64;
    fn generate_and_save_segmented(&self, image: &Image);
    fn get(&self, image_id: ImageId) -> Result<PreviewImage, PreviewStorageError>;
}

#[derive(Clone)]
pub struct PreviewServiceImpl {
    generator: Arc<dyn PreviewImageGenerator>,
    storage: Arc<dyn PreviewStorage>,
}

impl PreviewService for PreviewServiceImpl {
    fn generate_and_save_original(&self, image: &Image) -> f64 {
        let start = Instant::now();
        let (preview, scale) = self.generator.generate(image);
        let end = start.elapsed();
        tracing::debug!(elapsed = ?end, "preview: generate");

        self.storage.save_as_original(preview);
        scale
    }

    fn generate_and_save_segmented(&self, image: &Image) {
        let start = Instant::now();
        let (preview, _) = self.generator.generate(image);
        let end = start.elapsed();
        tracing::debug!(elapsed = ?end, "preview: generate");

        self.storage.save_as_segmented(preview);
    }

    fn get(&self, image_id: ImageId) -> Result<PreviewImage, PreviewStorageError> {
        let preview = self.storage.get(image_id)?;
        Ok(preview.as_ref().clone())
    }
}

impl PreviewServiceImpl {
    pub fn new(
        preview_generator: Arc<dyn PreviewImageGenerator>,
        preview_storage: Arc<dyn PreviewStorage>,
    ) -> Self {
        Self {
            generator: preview_generator,
            storage: preview_storage,
        }
    }
}
