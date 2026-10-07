use crate::application::{
    service::{completed_service::CompletedService, segment_service::SegmentService},
    usecase::save_usecase::{error::SaveUseCaseError, input::SaveInput, output::SaveOutput},
};

pub struct SaveUseCase<SS, CS>
where
    SS: SegmentService,
    CS: CompletedService,
{
    segment_service: SS,
    completed_service: CS,
}

impl<SS, CS> SaveUseCase<SS, CS>
where
    SS: SegmentService,
    CS: CompletedService,
{
    pub fn new(segment_service: SS, completed_service: CS) -> Self {
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
