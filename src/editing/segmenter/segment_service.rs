//! segment / undo / redo / save（resegment）の中核ロジック。
//!
//! 共通の流れ:
//! 1. `session_repo` から元画像の image_id を引く
//! 2. 元画像・SAM2 入力画像・編集セッションを各ストアから取得（取得で TTL も延長される）
//! 3. 編集セッションを clone して変更し、ストアに丸ごと保存し直す
//! 4. SAM2 で推論し、元画像サイズの「アルファ付き RGBA」を作る
//! 5. 新しい ImageId を振った `Image` を返す（プレビュー生成・保存はユースケース側の仕事）
//!
//! 各操作の違い:
//! | 操作 | 履歴の変更 | 推論 | 新マスクを履歴に保存 |
//! |---|---|---|---|
//! | segment | ポイント追加 | fast | する |
//! | undo | 1つ戻す（0点なら元画像を返す） | fast | しない（履歴にあるマスクを使う） |
//! | redo | 1つ進める | fast | しない |
//! | resegment | 変更なし（save 用） | high_quality | しない |
//!
//! 注意（並行性）: 同じセッションに対する同時リクエストはロックされていない。
//! 「取得 → clone → 変更 → 保存」の間に別リクエストが入ると後勝ちで更新が消える。
//! フロントは `session.busy` で1セッション1リクエストに直列化している前提。
//!
//! 注意（undo/redo の推論入力）: undo/redo 後の推論では、「戻った時点のマスク」を
//! prompt encoder に入れて再推論している。segment 時の推論（1つ前のマスクを入力）とは
//! 入力が異なるため、同じポイント列でも segment 直後と redo 後で結果が微妙に変わり得る。

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

        // ここで初めてプレビュー座標 → 元画像座標に変換される（セッションはまだ変更しない）。
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

        // 全部 undo して 0 点になった = 何も切り抜いていない状態なので、元画像をそのまま返す。
        // （SAM2 は 0 点では推論できない）。ID は新規発行。
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

        // 0 点で保存 = 元画像をそのまま保存。この場合だけ返す ImageId が元画像と同じになる。
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
