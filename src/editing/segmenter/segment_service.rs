use std::{ops::Deref, sync::Arc, time::Instant};

use crate::{
    editing::{
        segmenter::{
            error::SegmentServiceError,
            sam2_data::{SAM2InferenceContext, SAM2StaticContext},
            segmenter::ImageSegmenter,
        },
        session::{repository::repository::EditingSessionRepository, session::EditingSession},
    },
    image::{
        image::{id::ImageId, image::Image},
        original::original_repository::repository::OriginalImageRepository,
        segmenter_input::storage::storage::SegmenterInputImageStorage,
    },
    point::point::Point,
    session::{id::SessionId, repository::repository::SessionRepository},
};

pub trait SegmentService: Send + Sync {
    fn segment(&self, session_id: SessionId, point: Point) -> Result<Image, SegmentServiceError>;
    fn resegment(&self, session_id: SessionId) -> Result<Image, SegmentServiceError>;
    fn undo(&self, session_id: SessionId) -> Result<Image, SegmentServiceError>;
    fn redo(&self, session_id: SessionId) -> Result<Image, SegmentServiceError>;
}

#[derive(Clone)]
pub struct SegmentServiceImpl {
    session_repo: Arc<dyn SessionRepository>,
    segmenter: Arc<
        dyn ImageSegmenter<
                StaticContext = SAM2StaticContext,
                InferenceContext = SAM2InferenceContext,
            >,
    >,
    image_repo: Arc<dyn OriginalImageRepository>,
    editing_session_repo: Arc<
        dyn EditingSessionRepository<
                StaticContext = SAM2StaticContext,
                InferenceContext = SAM2InferenceContext,
            >,
    >,
    input_storage: Arc<dyn SegmenterInputImageStorage>,
}

impl SegmentService for SegmentServiceImpl {
    fn segment(&self, session_id: SessionId, point: Point) -> Result<Image, SegmentServiceError> {
        let service_start = Instant::now();

        println!("Input Point: {:?}", point);
        let session = self.session_repo.get(&session_id)?;
        let image_id = *session.image_id();

        let original_image = self.image_repo.get(&image_id)?;

        let input_image = self.input_storage.get(image_id)?;

        let editing_session = self.editing_session_repo.get(&session_id)?;
        let mut session = editing_session.as_ref().clone();
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
        self.editing_session_repo.save(&session_id, session);

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

        let editing_session = self.editing_session_repo.get(&session_id)?;
        let mut session = editing_session.as_ref().clone();
        session.undo()?;
        self.editing_session_repo.save(&session_id, session.clone());
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

        let editing_session = self.editing_session_repo.get(&session_id)?;
        let mut session = editing_session.as_ref().clone();
        session.redo()?;
        self.editing_session_repo.save(&session_id, session.clone());
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

impl SegmentServiceImpl {
    pub fn new(
        session_repository: Arc<dyn SessionRepository>,
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
        segmenter_input_storage: Arc<dyn SegmenterInputImageStorage>,
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
