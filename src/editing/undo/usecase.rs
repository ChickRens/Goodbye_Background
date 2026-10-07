use crate::application::{
    service::{preview_service::PreviewService, segment_service::SegmentService},
    usecase::undo_usecase::{
        error::UndoUseCaseError, undo_input::UndoInput, undo_output::UndoOutput,
    },
};

pub struct UndoUseCase<SS, PS>
where
    SS: SegmentService,
    PS: PreviewService,
{
    segment_service: SS,
    preview_service: PS,
}

impl<SS, PS> UndoUseCase<SS, PS>
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

    pub fn execute(&self, undo_input: UndoInput) -> Result<UndoOutput, UndoUseCaseError> {
        let session_id = undo_input.session_id();

        let segmented_image = self.segment_service.undo(session_id)?;
        self.preview_service
            .generate_and_save_segmented(&segmented_image);
        let segmented_image_id = *segmented_image.image_id();

        let output = UndoOutput::new(segmented_image_id);
        Ok(output)
    }
}
