use crate::application::{
    service::{preview_service::PreviewService, segment_service::SegmentService},
    usecase::segment_usecase::{
        error::SegmentUseCaseError, segment_input::SegmentInput, segment_output::SegmentOutput,
    },
};

pub struct SegmentUseCase<SS, PS>
where
    SS: SegmentService,
    PS: PreviewService,
{
    segment_service: SS,
    preview_service: PS,
}

impl<SS, PS> SegmentUseCase<SS, PS>
where
    SS: SegmentService,
    PS: PreviewService,
{
    pub fn new(segment_service: SS, preview_service: PS) -> Self {
        Self {
            segment_service,
            preview_service,
        }
    }

    pub fn execute(&self, input: SegmentInput) -> Result<SegmentOutput, SegmentUseCaseError> {
        let (session_id, point) = input.into_parts();

        let segmented_image = self.segment_service.segment(session_id, point)?;
        self.preview_service
            .generate_and_save_segmented(&segmented_image);

        let segmented_image_id = *segmented_image.image_id();
        let output = SegmentOutput::new(segmented_image_id);
        Ok(output)
    }
}
