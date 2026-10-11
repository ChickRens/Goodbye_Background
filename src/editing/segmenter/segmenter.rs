use crate::{
    application::{
        interface::image_segmenter::error::{SegmenterLoadingError, SegmenterRuntimeError},
        types::{segmented_image::SegmentedImage, segmenter_input_image::SegmenterInputImage},
    },
    domain::{entity::original_image::OriginalImage, value_object::point::Point},
};

pub trait ImageSegmenter {
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
