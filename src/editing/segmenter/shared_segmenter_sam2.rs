use std::sync::Arc;

use crate::{
    editing::segmenter::{
        error::{SegmenterLoadingError, SegmenterModelError, SegmenterRuntimeError},
        sam2_data::{SAM2InferenceContext, SAM2StaticContext},
        segmented_image::SegmentedImage,
        segmenter::ImageSegmenter,
        segmenter_sam2::Sam2Segmenter,
    },
    image::{
        original::original_image::OriginalImage,
        segmenter_input::segmenter_input_image::SegmenterInputImage,
    },
    point::point::Point,
};

#[derive(Clone)]
pub struct SharedSAM2Segmenter {
    segmenter: Arc<Sam2Segmenter>,
}

impl ImageSegmenter for SharedSAM2Segmenter {
    type StaticContext = SAM2StaticContext;
    type InferenceContext = SAM2InferenceContext;

    fn segment_fast(
        &self,
        input_image: &SegmenterInputImage,
        original_image: &OriginalImage,
        static_context: &Self::StaticContext,
        inference_context: &Self::InferenceContext,
        input_points: &[Point],
    ) -> Result<(Self::InferenceContext, SegmentedImage), SegmenterRuntimeError> {
        self.segmenter.segment_fast(
            input_image,
            original_image,
            static_context,
            inference_context,
            input_points,
        )
    }

    fn segment_high_quality(
        &self,
        input_image: &SegmenterInputImage,
        original_image: &OriginalImage,
        static_context: &Self::StaticContext,
        inference_context: &Self::InferenceContext,
        input_points: &[Point],
    ) -> Result<(Self::InferenceContext, SegmentedImage), SegmenterRuntimeError> {
        self.segmenter.segment_high_quality(
            input_image,
            original_image,
            static_context,
            inference_context,
            input_points,
        )
    }

    fn prepare_inference_context(
        &self,
        image: &SegmenterInputImage,
    ) -> Result<Self::InferenceContext, SegmenterLoadingError> {
        self.segmenter.prepare_inference_context(image)
    }

    fn prepare_static_context(
        &self,
        image: &SegmenterInputImage,
    ) -> Result<Self::StaticContext, SegmenterLoadingError> {
        self.segmenter.prepare_static_context(image)
    }
}

impl SharedSAM2Segmenter {
    pub fn new(model_dir: &str) -> Result<Self, SegmenterModelError> {
        // Self {
        //     sessions: Arc::new(RwLock::new(SessionRepositoryInMemory::new())),
        // }

        Ok(Self {
            segmenter: Arc::new(Sam2Segmenter::new(model_dir)?),
        })
    }
}
