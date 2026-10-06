use crate::application::{
    service::preview_service::PreviewService,
    usecase::get_preview_usecase::{
        error::GetPreviewUseCaseError, get_preview_input::GetPreviewInput,
        get_preview_output::GetPreviewOutput,
    },
};

pub struct GetPreviewUseCase<PS>
where
    PS: PreviewService,
{
    preview_service: PS,
}

impl<PS> GetPreviewUseCase<PS>
where
    PS: PreviewService,
{
    pub fn new(preview_service: PS) -> Self {
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
