//! プレビューのマスク。
//!
//! 持ち主確認 → `Created` か → 登録したフレームの特徴量があるか（無ければ `PREVIEW_FRAME_NOT_FOUND`）
//! → デコーダ → alpha（`worker/cutout/compositor.py` と同じ式）をフレームの大きさに拡大 → PNG。
//! 対象のスコア（`object_score_logits > 0`）も返す（ワーカーはこれが偽だと `subject_not_found` で失敗する）。
//! ジョブを変更しないのでロックは取らない。重い処理は `spawn_blocking`。回数は消費しない。

use std::{io::Cursor, sync::Arc};

use image::ImageFormat;
use tokio::task::spawn_blocking;

use crate::video_cutout::{
    job::{error::VideoJobError, phase::JobPhase, repository::VideoJobRepository},
    preview::{
        error::{PreviewError, PreviewInputError},
        feature_store::PreviewFeatureStore,
        image_math::{alpha_from_logits, alpha_to_gray},
        segmenter::PreviewSegmenter,
    },
    preview_mask::{error::PreviewMaskError, input::PreviewMaskInput, output::PreviewMaskOutput},
};

pub struct PreviewMaskUseCase {
    segmenter: Arc<dyn PreviewSegmenter>,
    repository: Arc<dyn VideoJobRepository>,
    features: Arc<PreviewFeatureStore>,
}

impl PreviewMaskUseCase {
    pub fn new(
        segmenter: Arc<dyn PreviewSegmenter>,
        repository: Arc<dyn VideoJobRepository>,
        features: Arc<PreviewFeatureStore>,
    ) -> Self {
        Self {
            segmenter,
            repository,
            features,
        }
    }

    pub async fn execute(
        &self,
        input: PreviewMaskInput,
    ) -> Result<PreviewMaskOutput, PreviewMaskError> {
        // 他人のジョブは存在しないものとして扱う
        let job = self
            .repository
            .get(&input.job_id)
            .filter(|job| job.owner_client_id == input.client_id)
            .ok_or(VideoJobError::NotFound)?;
        if job.phase != JobPhase::Created {
            return Err(VideoJobError::InvalidState.into());
        }
        let features = self
            .features
            .get(&job.id, input.time_ms)
            .ok_or(PreviewInputError::FrameNotFound)?;

        let segmenter = self.segmenter.clone();
        let points = input.points;
        let (png, object_present) = spawn_blocking(move || -> Result<_, PreviewError> {
            let decoded = segmenter.decode(&features, &points)?;
            let (width, height) = (features.frame_width, features.frame_height);
            let alpha = alpha_from_logits(&decoded.logits, width, height);
            let gray = alpha_to_gray(&alpha, width, height);
            let mut png = Vec::new();
            gray.write_to(&mut Cursor::new(&mut png), ImageFormat::Png)
                .map_err(|e| PreviewError::Inference(format!("PNG の書き出しに失敗: {e}")))?;
            Ok((png, decoded.object_present()))
        })
        .await
        .map_err(|e| PreviewError::Inference(format!("デコードのタスクが失敗: {e}")))??;

        Ok(PreviewMaskOutput {
            png,
            object_present,
        })
    }
}

#[cfg(test)]
mod preview_mask_usecase_test {
    use super::*;
    use crate::video_cutout::{job::id::VideoJobId, testing::*};

    #[tokio::test]
    async fn returns_gray_png_of_frame_size_from_logits() {
        let h = harness();
        let id = h.created_job().await;
        h.set_frame
            .execute(frame_input(id, png_bytes(40, 30)))
            .await
            .unwrap();

        let out = h.preview_mask.execute(mask_input(id)).await.unwrap();
        let img = image::load_from_memory_with_format(&out.png, ImageFormat::Png).unwrap();
        assert_eq!(img.color(), image::ColorType::L8);
        let gray = img.to_luma8();
        assert_eq!(gray.dimensions(), (40, 30));
        // 偽のデコーダは左半分を前景（ロジット +10）、右半分を背景（-10）にする
        assert_eq!(gray.get_pixel(0, 15).0, [255]);
        assert_eq!(gray.get_pixel(39, 15).0, [0]);
        assert_eq!(h.preview.decoded_points(), vec![mask_input(id).points]);
        assert!(out.object_present);
        // 回数は消費しない
        assert_eq!(h.remaining().await, 3);
    }

    #[tokio::test]
    async fn reports_absent_object_from_score() {
        let h = harness();
        let id = h.created_job().await;
        h.set_frame
            .execute(frame_input(id, png_bytes(4, 4)))
            .await
            .unwrap();
        h.preview.set_object_score(-1.0);
        let out = h.preview_mask.execute(mask_input(id)).await.unwrap();
        assert!(!out.object_present);
    }

    #[tokio::test]
    async fn without_frame_is_frame_not_found() {
        let h = harness();
        let id = h.created_job().await;
        let err = h.preview_mask.execute(mask_input(id)).await.unwrap_err();
        assert_eq!(err, PreviewInputError::FrameNotFound.into());
    }

    #[tokio::test]
    async fn started_job_is_invalid_state_even_with_frame() {
        let h = harness();
        let id = h.created_job().await;
        h.set_frame
            .execute(frame_input(id, png_bytes(4, 4)))
            .await
            .unwrap();
        h.start.execute(start_input(id)).await.unwrap();
        let err = h.preview_mask.execute(mask_input(id)).await.unwrap_err();
        assert_eq!(err, VideoJobError::InvalidState.into());
    }

    #[tokio::test]
    async fn other_client_or_unknown_job_is_not_found() {
        let h = harness();
        let id = h.created_job().await;
        h.set_frame
            .execute(frame_input(id, png_bytes(4, 4)))
            .await
            .unwrap();
        let mut input = mask_input(id);
        input.client_id = uuid::Uuid::new_v4();
        let err = h.preview_mask.execute(input).await.unwrap_err();
        assert_eq!(err, VideoJobError::NotFound.into());
        let err = h
            .preview_mask
            .execute(mask_input(VideoJobId::new_random()))
            .await
            .unwrap_err();
        assert_eq!(err, VideoJobError::NotFound.into());
    }

    #[tokio::test]
    async fn decodes_features_of_the_requested_time() {
        let h = harness();
        let id = h.created_job().await;
        h.set_frame.execute(frame_input(id, png_bytes(40, 30))).await.unwrap();
        let mut later = frame_input(id, png_bytes(20, 10));
        later.time_ms = 4_300;
        h.set_frame.execute(later).await.unwrap();

        let mut input = mask_input(id);
        input.time_ms = 4_300;
        let out = h.preview_mask.execute(input).await.unwrap();
        let img = image::load_from_memory_with_format(&out.png, ImageFormat::Png).unwrap();
        assert_eq!((img.width(), img.height()), (20, 10)); // 4_300 のフレームの大きさ
    }

    #[tokio::test]
    async fn unregistered_time_is_frame_not_found() {
        let h = harness();
        let id = h.created_job().await;
        h.set_frame.execute(frame_input(id, png_bytes(4, 4))).await.unwrap();
        let mut input = mask_input(id);
        input.time_ms = 4_300;
        let err = h.preview_mask.execute(input).await.unwrap_err();
        assert_eq!(err, PreviewInputError::FrameNotFound.into());
    }
}
