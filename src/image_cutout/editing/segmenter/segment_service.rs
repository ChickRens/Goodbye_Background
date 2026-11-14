//! segment / undo / redo / save（resegment）の中核ロジック。
//!
//! 共通の流れ:
//! 1. `session_repo` から元画像の image_id を引く
//! 2. 元画像・SAM2 入力画像・編集セッションを各ストアから取得（取得で TTL も延長される）
//! 3. 編集セッションを clone して変更し、ストアに丸ごと保存し直す
//! 4. SAM2 で推論し、元画像サイズの「アルファ付き RGBA」を作る
//! 5. 新しい ImageId を振った `Image` を返す（プレビュー生成・保存はユースケース側の仕事）
//!    segment/undo/redo の結果は **プレビュー解像度**（表示にしか使わないため）、
//!    resegment（save 用）の結果だけが元画像の解像度
//!
//! 各操作の違い:
//! | 操作 | 履歴の変更 | 推論 | 新マスクを履歴に保存 |
//! |---|---|---|---|
//! | segment | ポイント追加 | fast | する |
//! | undo | 1つ戻す（0点なら元画像を返す） | fast | しない（履歴にあるマスクを使う） |
//! | redo | 1つ進める | fast | しない |
//! | resegment | 変更なし（save 用） | high_quality | しない |
//!
//! 並行性: 「取得 → clone → 変更 → 保存」は `EditingSessionLocks` でセッション単位に直列化している
//! （ロック無しだと同時リクエストで後勝ちになり、片方のポイントが消える）。
//! ロック順は「セッションロック → ONNX セッションの Mutex」の一方向のみ（逆順で取らないこと）。
//!
//! 注意（undo/redo の推論入力）: undo/redo 後の推論では、「戻った時点のマスク」を
//! prompt encoder に入れて再推論している。segment 時の推論（1つ前のマスクを入力）とは
//! 入力が異なるため、同じポイント列でも segment 直後と redo 後で結果が微妙に変わり得る。

use std::{sync::Arc, sync::PoisonError, time::Instant};

use crate::{
    image_cutout::editing::{
        segmenter::{
            error::SegmentServiceError,
            sam2_data::{SAM2InferenceContext, SAM2StaticContext},
            segmenter::ImageSegmenter,
        },
        session::{
            lock::EditingSessionLocks, repository::repository::EditingSessionRepository,
            session::EditingSession,
        },
    },
    image_cutout::image::{
        image::{id::ImageId, image::Image},
        original::original_repository::repository::OriginalImageRepository,
        preview::generator::generator::PreviewImageGenerator,
        segmenter_input::storage::storage::SegmenterInputImageStorage,
    },
    image_cutout::point::point::Point,
    image_cutout::session::{id::SessionId, repository::repository::SessionRepository},
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
    session_locks: Arc<EditingSessionLocks>,
    /// segment/undo/redo の切り抜きをプレビュー解像度で行うために、元画像を縮小する。
    preview_generator: Arc<dyn PreviewImageGenerator>,
}

impl SegmentService for SegmentServiceImpl {
    fn segment(&self, session_id: SessionId, point: Point) -> Result<Image, SegmentServiceError> {
        // 取得 → 変更 → 保存の間、同じセッションへの他の操作を待たせる（lost update 防止）。
        // poison されていても守っているのは `()` なので続行してよい（実データは毎回ストアから読む）。
        let lock = self.session_locks.lock_for(session_id);
        let _guard = lock.lock().unwrap_or_else(PoisonError::into_inner);
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

        // ここで初めてプレビュー座標 → 元画像座標に変換される（セッションはまだ変更しない）。
        let input_points = session.points_with(point.clone());
        println!("Scaled Points: {:?}", input_points);

        // 表示用なのでプレビュー解像度で切り抜く（フル解像度だと 12MP 画像で約 200ms かかっていた）。
        let output_image = self.preview_generator.resize_to_preview(original_image.image());

        let start = Instant::now();
        let (new_context, segmented_image) = self.segmenter.segment_fast(
            &input_image,
            &original_image,
            static_context,
            inference_context,
            &input_points,
            &output_image,
        )?;
        let end = start.elapsed();
        println!("Segment: {:?}", end);

        // 推論が成功したときだけ履歴に確定する（失敗時はセッションが変わらない）。
        session.apply_edit(point, new_context);
        self.editing_session_repo.save(&session_id, session);

        let (segmented_image_data, size) = segmented_image.into_image_and_size();
        let segmented_image_id = ImageId::new();

        let end = service_start.elapsed();
        println!("Segment Service: {:?}", end);

        Ok(Image::new(segmented_image_data, segmented_image_id, size))
    }

    fn undo(&self, session_id: SessionId) -> Result<Image, SegmentServiceError> {
        // 取得 → 変更 → 保存の間、同じセッションへの他の操作を待たせる（lost update 防止）。
        // poison されていても守っているのは `()` なので続行してよい（実データは毎回ストアから読む）。
        let lock = self.session_locks.lock_for(session_id);
        let _guard = lock.lock().unwrap_or_else(PoisonError::into_inner);
        let session = self.session_repo.get(&session_id)?;
        let image_id = *session.image_id();

        let original_image = self.image_repo.get(&image_id)?;
        // 表示用なのでプレビュー解像度で切り抜く。
        let output_image = self.preview_generator.resize_to_preview(original_image.image());

        let input_image = self.input_storage.get(image_id)?;

        let editing_session = self.editing_session_repo.get(&session_id)?;
        let mut session = editing_session.as_ref().clone();
        session.undo()?;
        self.editing_session_repo.save(&session_id, session.clone());
        let input_points = session.points();

        // 全部 undo して 0 点になった = 何も切り抜いていない状態なので、元画像（プレビュー解像度）を返す。
        // （SAM2 は 0 点では推論できない）。ID は新規発行。
        if input_points.is_empty() {
            let (data, _, size) = output_image.into_data();
            return Ok(Image::new(data, ImageId::new(), size));
        }

        let static_context = session.static_context();
        let inference_context = session.inference_context();

        let (_, segmented_image) = self.segmenter.segment_fast(
            &input_image,
            &original_image,
            static_context,
            inference_context,
            &input_points,
            &output_image,
        )?;

        let (segmented_image_data, size) = segmented_image.into_image_and_size();
        let segmented_image_id = ImageId::new();

        Ok(Image::new(segmented_image_data, segmented_image_id, size))
    }

    fn resegment(&self, session_id: SessionId) -> Result<Image, SegmentServiceError> {
        // 取得 → 変更 → 保存の間、同じセッションへの他の操作を待たせる（lost update 防止）。
        // poison されていても守っているのは `()` なので続行してよい（実データは毎回ストアから読む）。
        let lock = self.session_locks.lock_for(session_id);
        let _guard = lock.lock().unwrap_or_else(PoisonError::into_inner);
        let session = self.session_repo.get(&session_id)?;
        let image_id = *session.image_id();
        let original_image = self.image_repo.get(&image_id)?;

        let input_image = self.input_storage.get(image_id)?;
        let session = self.editing_session_repo.get(&session_id)?;
        let input_points = session.points();

        // 0 点で保存 = 元画像をそのまま保存。この場合だけ返す ImageId が元画像と同じになる。
        if input_points.is_empty() {
            return Ok(original_image.image().clone());
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
        // 取得 → 変更 → 保存の間、同じセッションへの他の操作を待たせる（lost update 防止）。
        // poison されていても守っているのは `()` なので続行してよい（実データは毎回ストアから読む）。
        let lock = self.session_locks.lock_for(session_id);
        let _guard = lock.lock().unwrap_or_else(PoisonError::into_inner);
        let session = self.session_repo.get(&session_id)?;
        let image_id = *session.image_id();

        let original_image = self.image_repo.get(&image_id)?;
        // 表示用なのでプレビュー解像度で切り抜く。
        let output_image = self.preview_generator.resize_to_preview(original_image.image());

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
            &output_image,
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
        session_locks: Arc<EditingSessionLocks>,
        preview_generator: Arc<dyn PreviewImageGenerator>,
    ) -> Self {
        Self {
            session_repo: session_repository,
            segmenter: image_segmenter,
            image_repo: image_repository,
            editing_session_repo: editing_session_repository,
            input_storage: segmenter_input_storage,
            session_locks,
            preview_generator,
        }
    }
}

#[cfg(test)]
mod segment_service_test {
    use std::{sync::Arc, thread, time::Duration, time::Instant};

    use super::{SegmentService, SegmentServiceImpl};
    use crate::{
        image_cutout::editing::{
            segmenter::{
                error::{SegmenterLoadingError, SegmenterRuntimeError},
                sam2_data::{SAM2InferenceContext, SAM2StaticContext},
                segmented_image::SegmentedImage,
                segmenter::ImageSegmenter,
            },
            session::{
                inference_context_history::InferenceContextHistory,
                lock::EditingSessionLocks,
                repository::{
                    repository::EditingSessionRepository,
                    repository_impl::SAM2EditingSessionRepository,
                },
                session::{CommonEditingSession, EditingSession},
            },
        },
        image_cutout::image::{
            image::{id::ImageId, image::Image, raw_data::ImageData, size::ImageSize},
            preview::generator::generator_impl::WebPPreviewImageGenerator,
            original::{
                original_image::OriginalImage,
                original_repository::{
                    repository::OriginalImageRepository,
                    repository_impl::OriginalImageRepositoryInMemory,
                },
            },
            segmenter_input::{
                segmenter_input_image::SegmenterInputImage,
                storage::{
                    storage::SegmenterInputImageStorage,
                    storage_impl::SegmenterInputStorageInMemory,
                },
            },
        },
        maintenance::deleting_expired_entries::clock::clock_impl::FakeClock,
        image_cutout::point::{
            coordinate::Coordinate,
            point::{Point, PointLabel},
            point_history::PointHistory,
        },
        image_cutout::session::{
            id::SessionId,
            repository::{repository::SessionRepository, repository_impl::SessionRepositoryInMemory},
            session::Session,
        },
    };

    /// 推論に時間がかかるふりをするだけの偽セグメンタ（ONNX モデル不要）。
    struct SlowFakeSegmenter;

    impl ImageSegmenter for SlowFakeSegmenter {
        type StaticContext = SAM2StaticContext;
        type InferenceContext = SAM2InferenceContext;

        fn prepare_inference_context(
            &self,
            _image: &SegmenterInputImage,
        ) -> Result<SAM2InferenceContext, SegmenterLoadingError> {
            Ok(SAM2InferenceContext::default())
        }

        fn prepare_static_context(
            &self,
            _image: &SegmenterInputImage,
        ) -> Result<SAM2StaticContext, SegmenterLoadingError> {
            Ok(SAM2StaticContext::default())
        }

        fn segment_fast(
            &self,
            _input_image: &SegmenterInputImage,
            _original_image: &OriginalImage,
            _static_context: &SAM2StaticContext,
            _inference_context: &SAM2InferenceContext,
            _input_points: &[Point],
            output_image: &Image,
        ) -> Result<(SAM2InferenceContext, SegmentedImage), SegmenterRuntimeError> {
            thread::sleep(Duration::from_millis(200));
            Ok((
                SAM2InferenceContext::default(),
                SegmentedImage::new(
                    output_image.image_data().clone(),
                    output_image.image_size().clone(),
                ),
            ))
        }

        fn segment_high_quality(
            &self,
            input_image: &SegmenterInputImage,
            original_image: &OriginalImage,
            static_context: &SAM2StaticContext,
            inference_context: &SAM2InferenceContext,
            input_points: &[Point],
        ) -> Result<(SAM2InferenceContext, SegmentedImage), SegmenterRuntimeError> {
            self.segment_fast(
                input_image,
                original_image,
                static_context,
                inference_context,
                input_points,
                original_image.image(),
            )
        }
    }

    fn make_service_with_session() -> (
        SegmentServiceImpl,
        Arc<SAM2EditingSessionRepository>,
        SessionId,
    ) {
        let clock = FakeClock::new(Instant::now());
        let session_repo = Arc::new(SessionRepositoryInMemory::new(clock.clone()));
        let image_repo = Arc::new(OriginalImageRepositoryInMemory::new(Arc::new(clock.clone())));
        let input_storage = Arc::new(SegmenterInputStorageInMemory::new(clock.clone()));
        let editing_session_repo = Arc::new(SAM2EditingSessionRepository::new(clock.clone()));

        let image_id = ImageId::new();
        let image = Image::new(
            ImageData::new(vec![255; 8 * 8 * 4]),
            image_id,
            ImageSize::new(8, 8).unwrap(),
        );
        image_repo.save(OriginalImage::new(image.clone()));
        input_storage.save(SegmenterInputImage::new(image));

        let session_id = SessionId::new();
        session_repo.save(Session::new(session_id, image_id));
        editing_session_repo.save(
            &session_id,
            CommonEditingSession::new(
                PointHistory::new(10),
                SAM2StaticContext::default(),
                InferenceContextHistory::new(10, SAM2InferenceContext::default()),
                1.0,
            ),
        );

        let service = SegmentServiceImpl::new(
            session_repo,
            Arc::new(SlowFakeSegmenter),
            image_repo,
            editing_session_repo.clone(),
            input_storage,
            Arc::new(EditingSessionLocks::new(clock)),
            Arc::new(WebPPreviewImageGenerator::new(4)),
        );
        (service, editing_session_repo, session_id)
    }

    // 同じセッションへの segment が同時に来ても、両方のポイントが履歴に残ること（lost update しない）。
    #[test]
    fn concurrent_segments_on_same_session_keep_all_points() {
        let (service, editing_session_repo, session_id) = make_service_with_session();

        thread::scope(|scope| {
            for x in [0, 1] {
                let service = &service;
                scope.spawn(move || {
                    let point = Point::new(Coordinate::new(x, 0), PointLabel::FOREGROUND);
                    service.segment(session_id, point).unwrap();
                });
            }
        });

        let session = editing_session_repo.get(&session_id).unwrap();
        assert_eq!(session.points().len(), 2);
    }

    // segment の結果は表示用なので、元画像（8x8）ではなくプレビュー解像度（長辺4 → 4x4）で作ること。
    #[test]
    fn segment_cuts_out_at_preview_resolution() {
        let (service, _, session_id) = make_service_with_session();
        let point = Point::new(Coordinate::new(1, 1), PointLabel::FOREGROUND);

        let image = service.segment(session_id, point).unwrap();

        assert_eq!(image.image_size(), &ImageSize::new(4, 4).unwrap());
    }
}
