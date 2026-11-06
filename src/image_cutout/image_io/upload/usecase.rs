//! アップロードユースケース。
//!
//! 1. バイト列をデコード（形式判定・EXIF 回転・サイズ検証）→ 生 RGBA
//! 2. 新しい SessionId を発行し、Session(session_id → image_id) を保存
//! 3. 元画像のプレビュー（長辺600px WebP）を生成・保存し、座標変換倍率を得る
//! 4. 元画像を保存
//!
//! SAM2 の準備はここでは行わない（`App::upload` が続けて `PrepareSegmentUseCase` を呼ぶ）。

use std::sync::Arc;

use crate::{
    image_cutout::image::{
        original::{
            original_image::OriginalImage, original_repository::repository::OriginalImageRepository,
        },
        preview::service::PreviewService,
    },
    image_cutout::image_io::upload::{
        error::UploadUseCaseError, image_loader::loader::ImageLoader, input::UploadInput,
        output::UploadOutput,
    },
    image_cutout::session::{id::SessionId, repository::repository::SessionRepository, session::Session},
};

pub struct UploadUseCase {
    session_repo: Arc<dyn SessionRepository>,
    image_repo: Arc<dyn OriginalImageRepository>,
    loader: Arc<dyn ImageLoader>,
    preview_service: Arc<dyn PreviewService>,
}

impl UploadUseCase {
    pub fn new(
        session_repository: Arc<dyn SessionRepository>,
        image_repository: Arc<dyn OriginalImageRepository>,
        image_loader: Arc<dyn ImageLoader>,
        preview_service: Arc<dyn PreviewService>,
    ) -> Self {
        Self {
            session_repo: session_repository,
            image_repo: image_repository,
            loader: image_loader,
            preview_service,
        }
    }

    pub fn execute(&self, input: UploadInput) -> Result<UploadOutput, UploadUseCaseError> {
        let input_image = input.into_image_data();
        let loaded_image = self.loader.load(input_image)?;

        let image = loaded_image.into_image();
        let image_id = *image.image_id();

        let session_id = SessionId::new();
        let session = Session::new(session_id, image_id);
        self.session_repo.save(session);

        let scale = self.preview_service.generate_and_save_original(&image);

        let original_image = OriginalImage::new(image);
        self.image_repo.save(original_image);

        let output = UploadOutput::new(session_id, image_id, scale);

        Ok(output)
    }
}
