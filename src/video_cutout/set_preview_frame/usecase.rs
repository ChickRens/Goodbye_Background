//! プレビュー用フレームの登録。
//!
//! ジョブ単位のロックを持ったまま: 持ち主確認 → `Created` か → 画像を確かめてデコード → エンコード → 特徴量を保存。
//! ロックを持つのは、期限切れの片付け（同じロックを取る）とすれ違って、消えたジョブの特徴量が残らないようにするため。
//! デコードとエンコードは重いので `spawn_blocking` で動かす。回数は消費しない。

use std::{io::Cursor, sync::Arc};

use image::{ImageFormat, ImageReader, RgbImage};
use tokio::task::spawn_blocking;

use crate::video_cutout::{
    job::{
        error::VideoJobError, locks::VideoJobLocks, phase::JobPhase, repository::VideoJobRepository,
    },
    preview::{
        error::{PreviewError, PreviewInputError},
        feature_store::PreviewFeatureStore,
        segmenter::PreviewSegmenter,
    },
    set_preview_frame::{
        error::SetPreviewFrameError, input::SetPreviewFrameInput, output::SetPreviewFrameOutput,
    },
};

/// 登録できる画像の大きさの上限（バイト）。ルートの本文の上限（`router.rs`）にも使う。
pub const MAX_PREVIEW_IMAGE_BYTES: usize = 10 * 1024 * 1024;
/// 登録できる画像の長辺の上限（px）。
pub const MAX_PREVIEW_LONG_SIDE: u32 = 2000;

pub struct SetPreviewFrameUseCase {
    segmenter: Arc<dyn PreviewSegmenter>,
    repository: Arc<dyn VideoJobRepository>,
    locks: Arc<VideoJobLocks>,
    features: Arc<PreviewFeatureStore>,
}

impl SetPreviewFrameUseCase {
    pub fn new(
        segmenter: Arc<dyn PreviewSegmenter>,
        repository: Arc<dyn VideoJobRepository>,
        locks: Arc<VideoJobLocks>,
        features: Arc<PreviewFeatureStore>,
    ) -> Self {
        Self {
            segmenter,
            repository,
            locks,
            features,
        }
    }

    pub async fn execute(
        &self,
        input: SetPreviewFrameInput,
    ) -> Result<SetPreviewFrameOutput, SetPreviewFrameError> {
        // 無い ID のロックを作らない（消す人がいないため）。ロック内でもう一度取り直す
        if self.repository.get(&input.job_id).is_none() {
            return Err(VideoJobError::NotFound.into());
        }
        let lock = self.locks.lock_for(&input.job_id);
        let _guard = lock.lock_owned().await;

        // 他人のジョブは存在しないものとして扱う
        let job = self
            .repository
            .get(&input.job_id)
            .filter(|job| job.owner_client_id == input.client_id)
            .ok_or(VideoJobError::NotFound)?;
        if job.phase != JobPhase::Created {
            return Err(VideoJobError::InvalidState.into());
        }
        let time_ms = input.time_ms;
        if !(job.range.start_ms() <= time_ms && time_ms < job.range.end_ms()) {
            return Err(PreviewInputError::InvalidTime.into());
        }

        let segmenter = self.segmenter.clone();
        let image = input.image;
        let features = spawn_blocking(move || -> Result<_, SetPreviewFrameError> {
            let frame = decode_frame(&image)?;
            Ok(segmenter.encode(&frame)?)
        })
        .await
        .map_err(|e| PreviewError::Inference(format!("エンコードのタスクが失敗: {e}")))??;

        self.features.save(job.id, time_ms, Arc::new(features));
        Ok(SetPreviewFrameOutput)
    }
}

/// PNG / JPEG で、`MAX_PREVIEW_IMAGE_BYTES` 以下、長辺が `MAX_PREVIEW_LONG_SIDE` 以下のときだけ RGB にデコードする。
/// 大きさはデコードする前にヘッダーで確かめる（巨大な画像を展開しないため）。
fn decode_frame(bytes: &[u8]) -> Result<RgbImage, PreviewInputError> {
    if bytes.len() > MAX_PREVIEW_IMAGE_BYTES {
        return Err(PreviewInputError::InvalidImage);
    }
    let format = image::guess_format(bytes).map_err(|_| PreviewInputError::InvalidImage)?;
    if !matches!(format, ImageFormat::Png | ImageFormat::Jpeg) {
        return Err(PreviewInputError::InvalidImage);
    }
    let (width, height) = ImageReader::with_format(Cursor::new(bytes), format)
        .into_dimensions()
        .map_err(|_| PreviewInputError::InvalidImage)?;
    if width == 0 || height == 0 || width.max(height) > MAX_PREVIEW_LONG_SIDE {
        return Err(PreviewInputError::InvalidImage);
    }
    let image = ImageReader::with_format(Cursor::new(bytes), format)
        .decode()
        .map_err(|_| PreviewInputError::InvalidImage)?;
    Ok(image.to_rgb8())
}

#[cfg(test)]
mod set_preview_frame_usecase_test {
    use super::*;
    use crate::video_cutout::testing::*;

    #[tokio::test]
    async fn registers_features_with_frame_size() {
        let h = harness();
        let id = h.created_job().await;
        h.set_frame
            .execute(frame_input(id, png_bytes(40, 30)))
            .await
            .unwrap();
        let features = h.features.get(&id, START_MS).unwrap();
        assert_eq!((features.frame_width, features.frame_height), (40, 30));
        assert_eq!(h.preview.encoded(), 1);
        // 回数は消費しない
        assert_eq!(h.remaining().await, 3);
    }

    #[tokio::test]
    async fn accepts_jpeg_and_long_side_2000() {
        let h = harness();
        let id = h.created_job().await;
        h.set_frame
            .execute(frame_input(id, jpeg_bytes(2000, 10)))
            .await
            .unwrap();
        assert_eq!(h.features.get(&id, START_MS).unwrap().frame_width, 2000);
    }

    #[tokio::test]
    async fn rejects_too_large_or_broken_or_other_formats() {
        let h = harness();
        let id = h.created_job().await;
        let mut gif = Vec::new();
        image::RgbImage::new(4, 4)
            .write_to(&mut Cursor::new(&mut gif), ImageFormat::Gif)
            .unwrap();
        for bytes in [
            png_bytes(2001, 10),
            png_bytes(10, 2001),
            b"not an image".to_vec(),
            png_bytes(10, 10)[..40].to_vec(),
            gif,
            vec![0u8; MAX_PREVIEW_IMAGE_BYTES + 1],
        ] {
            let err = h
                .set_frame
                .execute(frame_input(id, bytes))
                .await
                .unwrap_err();
            assert_eq!(err, PreviewInputError::InvalidImage.into());
        }
        assert!(h.features.get(&id, START_MS).is_none());
        assert_eq!(h.preview.encoded(), 0);
    }

    #[tokio::test]
    async fn other_client_gets_not_found() {
        let h = harness();
        let id = h.created_job().await;
        let mut input = frame_input(id, png_bytes(4, 4));
        input.client_id = uuid::Uuid::new_v4();
        let err = h.set_frame.execute(input).await.unwrap_err();
        assert_eq!(err, VideoJobError::NotFound.into());
        let err = h
            .set_frame
            .execute(frame_input(
                crate::video_cutout::job::id::VideoJobId::new_random(),
                png_bytes(4, 4),
            ))
            .await
            .unwrap_err();
        assert_eq!(err, VideoJobError::NotFound.into());
    }

    #[tokio::test]
    async fn started_job_is_invalid_state() {
        let h = started_job().await;
        let err = h
            .set_frame
            .execute(frame_input(h.id, png_bytes(4, 4)))
            .await
            .unwrap_err();
        assert_eq!(err, VideoJobError::InvalidState.into());
    }

    fn frame_at(id: crate::video_cutout::job::id::VideoJobId, time_ms: u32, w: u32) -> SetPreviewFrameInput {
        let mut input = frame_input(id, png_bytes(w, 4));
        input.time_ms = time_ms;
        input
    }

    #[tokio::test]
    async fn registers_each_time_separately() {
        let h = harness();
        let id = h.created_job().await;
        h.set_frame.execute(frame_at(id, START_MS, 10)).await.unwrap();
        h.set_frame.execute(frame_at(id, 4_300, 20)).await.unwrap();
        assert_eq!(h.features.get(&id, START_MS).unwrap().frame_width, 10);
        assert_eq!(h.features.get(&id, 4_300).unwrap().frame_width, 20);
    }

    #[tokio::test]
    async fn rejects_time_outside_range() {
        let h = harness();
        let id = h.created_job().await;
        // 範囲は 1_000..9_000（create_input）
        for t in [999, 9_000, 20_000] {
            let err = h.set_frame.execute(frame_at(id, t, 4)).await.unwrap_err();
            assert_eq!(err, PreviewInputError::InvalidTime.into(), "{t}");
        }
        assert_eq!(h.preview.encoded(), 0);
    }

    #[tokio::test]
    async fn sixth_frame_replaces_the_least_recently_used_frame() {
        let h = harness();
        let id = h.created_job().await;
        for t in [1_000, 2_000, 3_000, 4_000, 5_000] {
            h.set_frame.execute(frame_at(id, t, 4)).await.unwrap();
        }
        h.set_frame.execute(frame_at(id, 6_000, 4)).await.unwrap();
        assert!(h.features.get(&id, 1_000).is_none()); // 一番長く使われていない 1 枚
        assert!(h.features.get(&id, 6_000).is_some());
    }
}
