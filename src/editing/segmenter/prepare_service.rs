use std::sync::Arc;

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

pub trait PrepareSegmentService {
    fn prepare(
        &self,
        session_id: SessionId,
        image_id: ImageId,
        scale: f64,
    ) -> Result<(), PrepareServiceError>;
}

pub struct PrepareSegmentServiceImpl<IS, IR, ESR, SG, SS>
where
    IS: ImageSegmenter,
    IR: OriginalImageRepository,
    ESR: EditingSessionRepository,
    SG: SegmenterInputImageGenerator,
    SS: SegmenterInputImageStorage,
{
    segmenter: IS,
    image_repo: IR,
    session_repo: ESR,
    generator: SG,
    storage: SS,
    config_max_history: usize,
}

impl<IS, IR, ESR, SG, SS> PrepareSegmentService for PrepareSegmentServiceImpl<IS, IR, ESR, SG, SS>
where
    IS: ImageSegmenter,
    IR: OriginalImageRepository,
    ESR: EditingSessionRepository<
            StaticContext = IS::StaticContext,
            InferenceContext = IS::InferenceContext,
        >,
    SG: SegmenterInputImageGenerator,
    SS: SegmenterInputImageStorage,
{
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

impl<IS, IR, ESR, SG, SS> PrepareSegmentServiceImpl<IS, IR, ESR, SG, SS>
where
    IS: ImageSegmenter,
    IR: OriginalImageRepository,
    ESR: EditingSessionRepository<
            StaticContext = IS::StaticContext,
            InferenceContext = IS::InferenceContext,
        >,
    SG: SegmenterInputImageGenerator,
    SS: SegmenterInputImageStorage,
{
    pub fn new(
        image_segmenter: IS,
        image_repository: IR,
        editing_session_repository: ESR,
        segmenter_input_generator: SG,
        segmenter_input_storage: SS,
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

#[derive(Clone)]
pub struct SharedPrepareSegmentService<IS, IR, ESR, SG, SS>
where
    IS: ImageSegmenter,
    IR: OriginalImageRepository,
    ESR: EditingSessionRepository<
            StaticContext = IS::StaticContext,
            InferenceContext = IS::InferenceContext,
        >,
    SG: SegmenterInputImageGenerator,
    SS: SegmenterInputImageStorage,
{
    service: Arc<PrepareSegmentServiceImpl<IS, IR, ESR, SG, SS>>,
}

impl<IS, IR, ESR, SG, SS> PrepareSegmentService for SharedPrepareSegmentService<IS, IR, ESR, SG, SS>
where
    IS: ImageSegmenter,
    IR: OriginalImageRepository,
    ESR: EditingSessionRepository<
            StaticContext = IS::StaticContext,
            InferenceContext = IS::InferenceContext,
        >,
    SG: SegmenterInputImageGenerator,
    SS: SegmenterInputImageStorage,
{
    fn prepare(
        &self,
        session_id: SessionId,
        image_id: ImageId,
        scale: f64,
    ) -> Result<(), PrepareServiceError> {
        self.service.prepare(session_id, image_id, scale)
    }
}

impl<IS, IR, ESR, SG, SS> SharedPrepareSegmentService<IS, IR, ESR, SG, SS>
where
    IS: ImageSegmenter,
    IR: OriginalImageRepository,
    ESR: EditingSessionRepository<
            StaticContext = IS::StaticContext,
            InferenceContext = IS::InferenceContext,
        >,
    SG: SegmenterInputImageGenerator,
    SS: SegmenterInputImageStorage,
{
    pub fn new(
        image_segmenter: IS,
        image_repository: IR,
        editing_session_repository: ESR,
        segmenter_input_generator: SG,
        segmenter_input_storage: SS,
        max_history: usize,
    ) -> Self {
        Self {
            service: Arc::new(PrepareSegmentServiceImpl::new(
                image_segmenter,
                image_repository,
                editing_session_repository,
                segmenter_input_generator,
                segmenter_input_storage,
                max_history,
            )),
        }
    }
}
