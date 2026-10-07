use crate::application::{
    service::prepare_service::PrepareSegmentService,
    usecase::prepare_segment_usecase::{
        error::PrepareSegmentUseCaseError, prepare_segment_input::PrepareSegmentInput,
    },
};

pub struct PrepareSegmentUseCase<PS>
where
    PS: PrepareSegmentService,
{
    prepare_service: PS,
}

impl<PS> PrepareSegmentUseCase<PS>
where
    PS: PrepareSegmentService,
{
    pub fn new(prepare_service: PS) -> Self {
        Self { prepare_service }
    }

    pub fn execute(&self, input: PrepareSegmentInput) -> Result<(), PrepareSegmentUseCaseError> {
        let image_id = input.image_id();
        let session_id = input.session_id();
        let original_to_preview_scale = input.point_scale();

        self.prepare_service
            .prepare(session_id, image_id, original_to_preview_scale)?;
        Ok(())
    }
}
