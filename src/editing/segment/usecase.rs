use std::sync::Arc;

use crate::{
    editing::{
        segment::{error::SegmentUseCaseError, input::SegmentInput, output::SegmentOutput},
        segmenter::segment_service::SegmentService,
    },
    image::preview::service::PreviewService,
};

pub struct SegmentUseCase {
    segment_service: Arc<dyn SegmentService>,
    preview_service: Arc<dyn PreviewService>,
}

impl SegmentUseCase {
    pub fn new(
        segment_service: Arc<dyn SegmentService>,
        preview_service: Arc<dyn PreviewService>,
    ) -> Self {
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
