use std::sync::Arc;

use crate::{
    editing::{
        segmenter::segment_service::SegmentService,
        undo::{error::UndoUseCaseError, input::UndoInput, output::UndoOutput},
    },
    image::preview::service::PreviewService,
};

pub struct UndoUseCase {
    segment_service: Arc<dyn SegmentService>,
    preview_service: Arc<dyn PreviewService>,
}

impl UndoUseCase {
    pub fn new(
        segment_service: Arc<dyn SegmentService>,
        preview_service: Arc<dyn PreviewService>,
    ) -> Self {
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
