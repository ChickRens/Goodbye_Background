use std::sync::Arc;

use crate::editing::segmenter::sam2_data::{SAM2InferenceContext, SAM2StaticContext};
use crate::{
    editing::{
        segmenter::{error::PrepareServiceError, segmenter::ImageSegmenter},
        session::{
            inference_context_history::InferenceContextHistory,
            repository::repository::EditingSessionRepository, session::CommonEditingSession,
        },
    },
    image::{
        image::id::ImageId,
        original::original_repository::repository::OriginalImageRepository,
        segmenter_input::{
            generator::generator::SegmenterInputImageGenerator,
            storage::storage::SegmenterInputImageStorage,
        },
    },
    point::point_history::PointHistory,
    session::id::SessionId,
};

pub trait PrepareSegmentService: Send + Sync {
    fn prepare(
        &self,
        session_id: SessionId,
        image_id: ImageId,
        scale: f64,
    ) -> Result<(), PrepareServiceError>;
}

#[derive(Clone)]
pub struct PrepareSegmentServiceImpl {
    segmenter: Arc<
        dyn ImageSegmenter<
                StaticContext = SAM2StaticContext,
                InferenceContext = SAM2InferenceContext,
            >,
    >,
    image_repo: Arc<dyn OriginalImageRepository>,
    session_repo: Arc<
        dyn EditingSessionRepository<
                StaticContext = SAM2StaticContext,
                InferenceContext = SAM2InferenceContext,
            >,
    >,
    generator: Arc<dyn SegmenterInputImageGenerator>,
    storage: Arc<dyn SegmenterInputImageStorage>,
    config_max_history: usize,
}

impl PrepareSegmentService for PrepareSegmentServiceImpl {
    fn prepare(
        &self,
        session_id: SessionId,
        image_id: ImageId,
        scale: f64,
    ) -> Result<(), PrepareServiceError> {
        let original_image = self.image_repo.get(&image_id)?;
        let segmenter_input = self.generator.generate(&original_image);

        let static_context = self.segmenter.prepare_static_context(&segmenter_input)?;
        let inference_context = self.segmenter.prepare_inference_context(&segmenter_input)?;

        let point_history = PointHistory::new(self.config_max_history);
        let inference_context_history =
            InferenceContextHistory::new(self.config_max_history, inference_context);

        let editing_session = CommonEditingSession::new(
            point_history,
            static_context,
            inference_context_history,
            scale,
        );
        self.session_repo.save(&session_id, editing_session);

        self.storage.save(segmenter_input);
        Ok(())
    }
}

impl PrepareSegmentServiceImpl {
    pub fn new(
        image_segmenter: Arc<
            dyn ImageSegmenter<
                    StaticContext = SAM2StaticContext,
                    InferenceContext = SAM2InferenceContext,
                >,
        >,
        image_repository: Arc<dyn OriginalImageRepository>,
        editing_session_repository: Arc<
            dyn EditingSessionRepository<
                    StaticContext = SAM2StaticContext,
                    InferenceContext = SAM2InferenceContext,
                >,
        >,
        segmenter_input_generator: Arc<dyn SegmenterInputImageGenerator>,
        segmenter_input_storage: Arc<dyn SegmenterInputImageStorage>,
        max_history: usize,
    ) -> Self {
        Self {
            segmenter: image_segmenter,
            image_repo: image_repository,
            session_repo: editing_session_repository,
            generator: segmenter_input_generator,
            storage: segmenter_input_storage,
            config_max_history: max_history,
        }
    }
}
