use std::{ops::Deref, sync::Arc, time::Instant};

use crate::{
    application::{
        interface::{
            editing_session_repository::repository::EditingSessionRepository,
            image_segmenter::segmenter::ImageSegmenter,
            segmenter_input_image_storage::storage::SegmenterInputImageStorage,
        },
        service::error::SegmentServiceError,
        types::editing_session::session::EditingSession,
    },
    domain::{
        entity::image::Image,
        repository::{
            original_image_repository::repository::OriginalImageRepository,
            session_repository::repository::SessionRepository,
        },
        value_object::{
            image_id::image_id::ImageId, point::Point, session_id::session_id::SessionId,
        },
    },
};

pub trait SegmentService {
    fn segment(&self, session_id: SessionId, point: Point) -> Result<Image, SegmentServiceError>;
    fn resegment(&self, session_id: SessionId) -> Result<Image, SegmentServiceError>;
    fn undo(&self, session_id: SessionId) -> Result<Image, SegmentServiceError>;
    fn redo(&self, session_id: SessionId) -> Result<Image, SegmentServiceError>;
}

pub struct SegmentServiceImpl<SR, IS, IR, ESR, SS>
where
    SR: SessionRepository,
    IS: ImageSegmenter,
    IR: OriginalImageRepository,
    ESR: EditingSessionRepository,
    SS: SegmenterInputImageStorage,
{
    session_repo: SR,
    segmenter: IS,
    image_repo: IR,
    editing_session_repo: ESR,
    input_storage: SS,
}

impl<SR, IS, IR, ESR, SS, SC, IC> SegmentService for SegmentServiceImpl<SR, IS, IR, ESR, SS>
where
    SR: SessionRepository,
    IS: ImageSegmenter<StaticContext = SC, InferenceContext = IC>,
    IR: OriginalImageRepository,
    ESR: EditingSessionRepository<StaticContext = SC, InferenceContext = IC>,
    SS: SegmenterInputImageStorage,
{
    fn segment(&self, session_id: SessionId, point: Point) -> Result<Image, SegmentServiceError> {
        let service_start = Instant::now();

        println!("Input Point: {:?}", point);
        let session = self.session_repo.get(&session_id)?;
        let image_id = *session.image_id();

        let original_image = self.image_repo.get(&image_id)?;

        let input_image = self.input_storage.get(image_id)?;

        let mut session = self.editing_session_repo.get(&session_id)?;
        let static_context = session.static_context();
        let inference_context = session.inference_context();

        let input_points = session.points_with(point.clone());
        println!("Scaled Points: {:?}", input_points);

        let start = Instant::now();
        let (new_context, segmented_image) = self.segmenter.segment_fast(
            &input_image,
            &original_image,
            static_context,
            inference_context,
            &input_points,
        )?;
        let end = start.elapsed();
        println!("Segment: {:?}", end);

        session.apply_edit(point, new_context);

        let (segmented_image_data, size) = segmented_image.into_image_and_size();
        let segmented_image_id = ImageId::new();

        let end = service_start.elapsed();
        println!("Segment Service: {:?}", end);

        Ok(Image::new(segmented_image_data, segmented_image_id, size))
    }

    fn undo(&self, session_id: SessionId) -> Result<Image, SegmentServiceError> {
        let session = self.session_repo.get(&session_id)?;
        let image_id = *session.image_id();

        let guard = self.image_repo.get(&image_id)?;
        let original_image = guard.deref().clone();

        let input_image = self.input_storage.get(image_id)?;

        let mut session = self.editing_session_repo.get(&session_id)?;
        session.undo()?;
        let input_points = session.points();

        if input_points.is_empty() {
            let image = original_image.into_image();
            let (data, _, size) = image.into_data();
            let id = ImageId::new();

            let original_image = Image::new(data, id, size);
            return Ok(original_image);
        }

        let static_context = session.static_context();
        let inference_context = session.inference_context();

        let (_, segmented_image) = self.segmenter.segment_fast(
            &input_image,
            &original_image,
            static_context,
            inference_context,
            &input_points,
        )?;

        let (segmented_image_data, size) = segmented_image.into_image_and_size();
        let segmented_image_id = ImageId::new();

        Ok(Image::new(segmented_image_data, segmented_image_id, size))
    }

    fn resegment(&self, session_id: SessionId) -> Result<Image, SegmentServiceError> {
        let session = self.session_repo.get(&session_id)?;
        let image_id = *session.image_id();
        let guard = self.image_repo.get(&image_id)?;
        let original_image = guard.deref().clone();

        let input_image = self.input_storage.get(image_id)?;
        let session = self.editing_session_repo.get(&session_id)?;
        let input_points = session.points();

        if input_points.is_empty() {
            return Ok(original_image.into_image());
        }

        let (_, segmented_image) = self.segmenter.segment_high_quality(
            &input_image,
            &original_image,
            session.static_context(),
            session.inference_context(),
            &input_points,
        )?;

        let (segmented_image_data, size) = segmented_image.into_image_and_size();
        Ok(Image::new(segmented_image_data, ImageId::new(), size))
    }

    fn redo(&self, session_id: SessionId) -> Result<Image, SegmentServiceError> {
        let session = self.session_repo.get(&session_id)?;
        let image_id = *session.image_id();

        let original_image = self.image_repo.get(&image_id)?;

        let input_image = self.input_storage.get(image_id)?;

        let mut session = self.editing_session_repo.get(&session_id)?;
        session.redo()?;
        let input_points = session.points();

        let static_context = session.static_context();
        let inference_context = session.inference_context();

        let (_, segmented_image) = self.segmenter.segment_fast(
            &input_image,
            &original_image,
            static_context,
            inference_context,
            &input_points,
        )?;

        let (segmented_image_data, size) = segmented_image.into_image_and_size();
        let segmented_image_id = ImageId::new();

        Ok(Image::new(segmented_image_data, segmented_image_id, size))
    }
}

impl<SR, IS, IR, ESR, SS> SegmentServiceImpl<SR, IS, IR, ESR, SS>
where
    SR: SessionRepository,
    IS: ImageSegmenter,
    IR: OriginalImageRepository,
    ESR: EditingSessionRepository<
            StaticContext = IS::StaticContext,
            InferenceContext = IS::InferenceContext,
        >,
    SS: SegmenterInputImageStorage,
{
    pub fn new(
        session_repository: SR,
        image_segmenter: IS,
        image_repository: IR,
        editing_session_repository: ESR,
        segmenter_input_storage: SS,
    ) -> Self {
        Self {
            session_repo: session_repository,
            segmenter: image_segmenter,
            image_repo: image_repository,
            editing_session_repo: editing_session_repository,
            input_storage: segmenter_input_storage,
        }
    }
}

#[derive(Clone)]
pub struct SharedSegmentService<SR, IS, IR, ESR, SS>
where
    SR: SessionRepository,
    IS: ImageSegmenter,
    IR: OriginalImageRepository,
    ESR: EditingSessionRepository<
            StaticContext = IS::StaticContext,
            InferenceContext = IS::InferenceContext,
        >,
    SS: SegmenterInputImageStorage,
{
    service: Arc<SegmentServiceImpl<SR, IS, IR, ESR, SS>>,
}

impl<SR, IS, IR, ESR, SS> SegmentService for SharedSegmentService<SR, IS, IR, ESR, SS>
where
    SR: SessionRepository,
    IS: ImageSegmenter,
    IR: OriginalImageRepository,
    ESR: EditingSessionRepository<
            StaticContext = IS::StaticContext,
            InferenceContext = IS::InferenceContext,
        >,
    SS: SegmenterInputImageStorage,
{
    fn segment(&self, session_id: SessionId, point: Point) -> Result<Image, SegmentServiceError> {
        self.service.segment(session_id, point)
    }

    fn resegment(&self, session_id: SessionId) -> Result<Image, SegmentServiceError> {
        self.service.resegment(session_id)
    }

    fn undo(&self, session_id: SessionId) -> Result<Image, SegmentServiceError> {
        self.service.undo(session_id)
    }

    fn redo(&self, session_id: SessionId) -> Result<Image, SegmentServiceError> {
        self.service.redo(session_id)
    }
}

impl<SR, IS, IR, ESR, SS> SharedSegmentService<SR, IS, IR, ESR, SS>
where
    SR: SessionRepository,
    IS: ImageSegmenter,
    IR: OriginalImageRepository,
    ESR: EditingSessionRepository<
            StaticContext = IS::StaticContext,
            InferenceContext = IS::InferenceContext,
        >,
    SS: SegmenterInputImageStorage,
{
    pub fn new(
        session_repository: SR,
        image_segmenter: IS,
        image_repository: IR,
        editing_session_repository: ESR,
        segmenter_input_storage: SS,
    ) -> Self {
        Self {
            service: Arc::new(SegmentServiceImpl::new(
                session_repository,
                image_segmenter,
                image_repository,
                editing_session_repository,
                segmenter_input_storage,
            )),
        }
    }
}
