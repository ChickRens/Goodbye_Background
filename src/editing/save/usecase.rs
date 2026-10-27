use std::sync::Arc;

use crate::{
    editing::{
        save::{error::SaveUseCaseError, input::SaveInput, output::SaveOutput},
        segmenter::segment_service::SegmentService,
    },
    image::completed::completed_service::CompletedService,
};

pub struct SaveUseCase {
    segment_service: Arc<dyn SegmentService>,
    completed_service: Arc<dyn CompletedService>,
}

impl SaveUseCase {
    pub fn new(
        segment_service: Arc<dyn SegmentService>,
        completed_service: Arc<dyn CompletedService>,
    ) -> Self {
        Self {
            segment_service,
            completed_service,
        }
    }

    pub fn execute(&self, save_input: SaveInput) -> Result<SaveOutput, SaveUseCaseError> {
        let session_id = save_input.session_id();

        let segmented_image = self.segment_service.resegment(session_id)?;
        let segmented_image_id = *segmented_image.image_id();

        self.completed_service.generate_and_save(&segmented_image);

        let output = SaveOutput::new(segmented_image_id);
        Ok(output)
    }
}
