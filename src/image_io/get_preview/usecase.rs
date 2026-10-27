use std::sync::Arc;

use crate::{
    image::preview::service::PreviewService,
    image_io::get_preview::{
        error::GetPreviewUseCaseError, input::GetPreviewInput, output::GetPreviewOutput,
    },
};

pub struct GetPreviewUseCase {
    preview_service: Arc<dyn PreviewService>,
}

impl GetPreviewUseCase {
    pub fn new(preview_service: Arc<dyn PreviewService>) -> Self {
        Self { preview_service }
    }

    pub fn execute(
        &self,
        input: GetPreviewInput,
    ) -> Result<GetPreviewOutput, GetPreviewUseCaseError> {
        let image_id = input.image_id();

        let preview = self.preview_service.get(image_id)?;

        let (data, _, size) = preview.into_data();

        let output = GetPreviewOutput::new(data.into_image(), size.clone());

        Ok(output)
    }
}
