use crate::{
    application::{
        interface::image_loader::loader::ImageLoader,
        service::preview_service::PreviewService,
        usecase::upload_usecase::{
            error::UploadUseCaseError, upload_input::UploadInput, upload_output::UploadOutput,
        },
    },
    domain::{
        entity::{original_image::OriginalImage, session::Session},
        repository::{
            original_image_repository::repository::OriginalImageRepository,
            session_repository::repository::SessionRepository,
        },
        value_object::session_id::session_id::SessionId,
    },
};

pub struct UploadUseCase<SR, IR, LD, PS>
where
    SR: SessionRepository,
    IR: OriginalImageRepository,
    LD: ImageLoader,
    PS: PreviewService,
{
    session_repo: SR,
    image_repo: IR,
    loader: LD,
    preview_service: PS,
}

impl<SR, IR, LD, PS> UploadUseCase<SR, IR, LD, PS>
where
    SR: SessionRepository,
    IR: OriginalImageRepository,
    LD: ImageLoader,
    PS: PreviewService,
{
    pub fn new(
        session_repository: SR,
        image_repository: IR,
        image_loader: LD,
        preview_service: PS,
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
