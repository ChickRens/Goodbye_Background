use std::sync::Arc;

use crate::editing::{
    prepare_segment::{error::PrepareSegmentUseCaseError, input::PrepareSegmentInput},
    segmenter::prepare_service::PrepareSegmentService,
};

pub struct PrepareSegmentUseCase {
    prepare_service: Arc<dyn PrepareSegmentService>,
}

impl PrepareSegmentUseCase {
    pub fn new(prepare_service: Arc<dyn PrepareSegmentService>) -> Self {
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
