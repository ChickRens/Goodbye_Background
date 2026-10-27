use crate::{
    editing::segmenter::error::{SegmenterLoadingError, SegmenterRuntimeError},
    editing::segmenter::segmented_image::SegmentedImage,
    image::{
        original::original_image::OriginalImage,
        segmenter_input::segmenter_input_image::SegmenterInputImage,
    },
    point::point::Point,
};

pub trait ImageSegmenter: Send + Sync {
    type StaticContext;
    type InferenceContext;

    fn prepare_inference_context(
        &self,
        image: &SegmenterInputImage,
    ) -> Result<Self::InferenceContext, SegmenterLoadingError>;
    fn prepare_static_context(
        &self,
        image: &SegmenterInputImage,
    ) -> Result<Self::StaticContext, SegmenterLoadingError>;
    fn segment_fast(
        &self,
        input_image: &SegmenterInputImage,
        original_image: &OriginalImage,
        static_context: &Self::StaticContext,
        inference_context: &Self::InferenceContext,
        input_points: &[Point],
    ) -> Result<(Self::InferenceContext, SegmentedImage), SegmenterRuntimeError>;
    fn segment_high_quality(
        &self,
        input_image: &SegmenterInputImage,
        original_image: &OriginalImage,
        static_context: &Self::StaticContext,
        inference_context: &Self::InferenceContext,
        input_points: &[Point],
    ) -> Result<(Self::InferenceContext, SegmentedImage), SegmenterRuntimeError>;
}
