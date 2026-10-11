use crate::{
    editing::{
        redo::{error::RedoUseCaseError, input::RedoInput, output::RedoOutput},
        segmenter::segment_service::SegmentService,
    },
    image::preview::service::PreviewService,
};

pub struct RedoUseCase<PS, SS>
where
    PS: PreviewService,
    SS: SegmentService,
{
    preview_service: PS,
    segment_service: SS,
}

impl<PS, SS> RedoUseCase<PS, SS>
where
    PS: PreviewService,
    SS: SegmentService,
{
    pub fn new(segment_service: SS, preview_service: PS) -> Self {
        Self {
            segment_service,
            preview_service,
        }
    }

    pub fn execute(&self, redo_input: RedoInput) -> Result<RedoOutput, RedoUseCaseError> {
        let session_id = redo_input.session_id();
        let segmented_image = self.segment_service.redo(session_id)?;

        self.preview_service
            .generate_and_save_segmented(&segmented_image);
        let segmented_image_id = *segmented_image.image_id();

        let output = RedoOutput::new(segmented_image_id);
        Ok(output)
    }
}
