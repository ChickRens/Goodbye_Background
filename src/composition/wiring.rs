//! 依存性の組み立て（手書きの DI）。全ユースケース・サービス・ストアはここで1回だけ生成される。
//!
//! 「この trait の実装はどれ？」「このストアは誰と共有されている？」と思ったらまずここを見る。
//! 各ストアは `Arc` で共有され、同じインスタンスが複数のサービス/ユースケースから参照される。
//!
//! HTTP ハンドラは `App` のメソッドだけを呼ぶ（ユースケースを直接触らない）。

use std::{sync::Arc, time::Instant};

use crate::{
    analytics::usage_tracking::usage_recorder::UsageRecorder,
    application::error::ApplicationError,
    config::{MAX_HISTORY, TTL_SEC},
    editing::{
        prepare_segment::{input::PrepareSegmentInput, usecase::PrepareSegmentUseCase},
        redo::{input::RedoInput, output::RedoOutput, usecase::RedoUseCase},
        save::{input::SaveInput, output::SaveOutput, usecase::SaveUseCase},
        segment::{input::SegmentInput, output::SegmentOutput, usecase::SegmentUseCase},
        segmenter::{
            prepare_service::{PrepareSegmentService, PrepareSegmentServiceImpl},
            segment_service::{SegmentService, SegmentServiceImpl},
            segmenter_sam2::Sam2Segmenter,
        },
        session::repository::repository_impl::SAM2EditingSessionRepository,
        undo::{input::UndoInput, output::UndoOutput, usecase::UndoUseCase},
    },
    image::{
        completed::{
            completed_service::{CompletedService, CompletedServiceImpl},
            generator::generator::WebPCompletedImageGenerator,
            storage::storage_impl::CompletedRepositoryInMemory,
        },
        original::original_repository::repository_impl::OriginalImageRepositoryInMemory,
        preview::{
            generator::generator_impl::WebPPreviewImageGenerator,
            service::{PreviewService, PreviewServiceImpl},
            storage::storage_impl::PreviewStorageInMemory,
        },
        segmenter_input::{
            generator::generator_impl::SAM2InputGenerator,
            storage::storage_impl::SegmenterInputStorageInMemory,
        },
    },
    image_io::{
        get_completed::{
            input::GetCompletedInput, output::GetCompletedOutput, usecase::GetCompletedUseCase,
        },
        get_preview::{
            input::GetPreviewInput, output::GetPreviewOutput, usecase::GetPreviewUseCase,
        },
        upload::{
            image_loader::loader_impl::FileImageLoader, input::UploadInput, output::UploadOutput,
            usecase::UploadUseCase,
        },
    },
    maintenance::deleting_expired_entries::{
        clock::clock_impl::RealClock,
        repository::DeleteExpiredRepository,
        usecase::{
            DeleteExpiredEntriesUseCase, ExpiringCompletedImageRepository,
            ExpiringOriginalImageRepository, ExpiringPreviewStorage, ExpiringSegmenterInputStorage,
            ExpiringSessionRepository,
        },
    },
    session::repository::repository_impl::SessionRepositoryInMemory,
};

/// アプリ全体のファサード。axum の `State<Arc<App>>` として全ハンドラに渡される。
///
/// 同期メソッド（upload/segment/undo/redo/save）は CPU 重い処理を含むので、
/// ハンドラ側で `spawn_blocking` 経由で呼ぶこと。
pub struct App {
    upload_usecase: UploadUseCase,
    prepare_segment_usecase: PrepareSegmentUseCase,
    segment_usecase: SegmentUseCase,
    undo_usecase: UndoUseCase,
    redo_usecase: RedoUseCase,
    save_usecase: SaveUseCase,
    get_completed_usecase: GetCompletedUseCase,
    get_preview_usecase: GetPreviewUseCase,
    delete_expired_usecase: DeleteExpiredEntriesUseCase,
    usage_recorder: Arc<dyn UsageRecorder>,
}

impl App {
    /// 全依存を組み立てる。
    ///
    /// 注意: SAM2 モデルの読み込みに失敗すると `Result` ではなく panic する（`expect`）。
    /// 戻り値の `Result` は現状 `Err` を返す経路が無い。
    pub fn new(usage_recorder: Arc<dyn UsageRecorder>) -> Result<Self, ApplicationError> {
        let clock = RealClock::new();

        // ---- ストア（全てインメモリ + TTL。サーバー再起動で全て消える） ----
        let session_repo = Arc::new(SessionRepositoryInMemory::new(clock.clone()));
        let image_repo = Arc::new(OriginalImageRepositoryInMemory::new(Arc::new(
            clock.clone(),
        )));
        let loader = FileImageLoader::new();
        // Sam2Segmenter は内部で ONNX セッションを Arc<Mutex<_>> で持つので、
        // 下で clone しても同じモデルインスタンスを共有する（推論は全リクエストで直列化される）。
        let segmenter =
            Sam2Segmenter::new("models").expect("Failed to load SAM2 model during app initialize");
        // SAM2 特徴量（1セッション約16MB）を保持する一番重いストア。GC 対象に必ず含めること。
        let editing_session_repo = Arc::new(SAM2EditingSessionRepository::new(clock.clone()));
        let preview_storage = Arc::new(PreviewStorageInMemory::new(clock.clone()));
        // プレビューは長辺 600px。フロントのクリック座標はこの解像度基準で送られてくる。
        let preview_generator = WebPPreviewImageGenerator::new(600);
        let input_storage = Arc::new(SegmenterInputStorageInMemory::new(clock.clone()));
        // SAM2 の image encoder は 1024x1024 固定入力。
        let input_generator = SAM2InputGenerator::new(1024, 1024);
        let completed_image_generator = WebPCompletedImageGenerator::new();
        let completed_image_repo = Arc::new(CompletedRepositoryInMemory::new(clock.clone()));

        // ---- サービス（複数ユースケースで共有するドメインロジック） ----
        let preview_service: Arc<dyn PreviewService> = Arc::new(PreviewServiceImpl::new(
            Arc::new(preview_generator),
            preview_storage.clone(),
        ));
        let prepare_service: Arc<dyn PrepareSegmentService> =
            Arc::new(PrepareSegmentServiceImpl::new(
                Arc::new(segmenter.clone()),
                image_repo.clone(),
                editing_session_repo.clone(),
                Arc::new(input_generator),
                input_storage.clone(),
                MAX_HISTORY,
            ));
        let segment_service: Arc<dyn SegmentService> = Arc::new(SegmentServiceImpl::new(
            session_repo.clone(),
            Arc::new(segmenter.clone()),
            image_repo.clone(),
            editing_session_repo.clone(),
            input_storage.clone(),
        ));
        let completed_service: Arc<dyn CompletedService> = Arc::new(CompletedServiceImpl::new(
            Arc::new(completed_image_generator),
            completed_image_repo.clone(),
        ));

        // ---- ユースケース（1 API = 1 ユースケース。prepare_segment だけは upload の後段） ----
        let upload_uc = UploadUseCase::new(
            session_repo.clone(),
            image_repo.clone(),
            Arc::new(loader),
            preview_service.clone(),
        );
        let prepare_segment_uc = PrepareSegmentUseCase::new(prepare_service);
        let segment_uc = SegmentUseCase::new(segment_service.clone(), preview_service.clone());
        let undo_uc = UndoUseCase::new(segment_service.clone(), preview_service.clone());
        let redo_uc = RedoUseCase::new(segment_service.clone(), preview_service.clone());
        let save_uc = SaveUseCase::new(segment_service.clone(), completed_service.clone());
        let get_completed_uc = GetCompletedUseCase::new(completed_service.clone());
        let get_image_uc = GetPreviewUseCase::new(preview_service.clone());
        // GC 対象のストア一覧。ストアを追加したらここにも渡すこと（渡し忘れ = メモリリーク）。
        let delete_expired_uc = DeleteExpiredEntriesUseCase::new(
            image_repo.clone() as Arc<dyn ExpiringOriginalImageRepository>,
            session_repo.clone() as Arc<dyn ExpiringSessionRepository>,
            completed_image_repo.clone() as Arc<dyn ExpiringCompletedImageRepository>,
            preview_storage.clone() as Arc<dyn ExpiringPreviewStorage>,
            input_storage.clone() as Arc<dyn ExpiringSegmenterInputStorage>,
            editing_session_repo.clone() as Arc<dyn DeleteExpiredRepository>,
            TTL_SEC,
        );

        Ok(App {
            upload_usecase: upload_uc,
            prepare_segment_usecase: prepare_segment_uc,
            segment_usecase: segment_uc,
            undo_usecase: undo_uc,
            redo_usecase: redo_uc,
            get_completed_usecase: get_completed_uc,
            save_usecase: save_uc,
            get_preview_usecase: get_image_uc,
            delete_expired_usecase: delete_expired_uc,
            usage_recorder,
        })
    }

    /// `/upload`: 画像デコード + セッション作成 + 元画像プレビュー生成（UploadUseCase）を行い、
    /// 続けて SAM2 の画像エンコード（PrepareSegmentUseCase。数百ms〜数秒かかる重い処理）を行う。
    ///
    /// 2つのユースケースを順に呼ぶだけでトランザクションは無い。
    /// 後半が失敗するとセッションと元画像だけが残る（TTL で消える）。
    pub fn upload(&self, input: UploadInput) -> Result<UploadOutput, ApplicationError> {
        let output = self.upload_usecase.execute(input)?;
        // 名前は "preview_to_original" だが、実際の値は preview = original * scale となる倍率
        // （= 600 / 元画像の長辺）。ポイントは `point / scale` で元画像座標に戻される。
        let preview_to_original_point_scale = output.scale();
        let (session_id, image_id) = output.session_id_and_image_id();

        let input = PrepareSegmentInput::new(image_id, session_id, preview_to_original_point_scale);
        self.prepare_segment_usecase.execute(input)?;
        Ok(output)
    }

    /// `/segment`: ポイントを追加して推論（高速版）し、切り抜き結果のプレビューを作る。
    pub fn segment(&self, input: SegmentInput) -> Result<SegmentOutput, ApplicationError> {
        Ok(self.segment_usecase.execute(input)?)
    }

    /// `/undo`: 最後のポイントを取り消して再推論する。ポイントが0個になったら元画像そのものを返す。
    pub fn undo(&self, input: UndoInput) -> Result<UndoOutput, ApplicationError> {
        Ok(self.undo_usecase.execute(input)?)
    }

    /// `/redo`: undo したポイントを戻して再推論する。
    pub fn redo(&self, input: RedoInput) -> Result<RedoOutput, ApplicationError> {
        Ok(self.redo_usecase.execute(input)?)
    }

    /// `/save`: 現在のポイントで高品質推論（エッジ平滑化あり）し、保存用 WebP を作る。
    /// 返す image_id は `/get-completed` 用（プレビューストアには入らない）。
    pub fn save(&self, input: SaveInput) -> Result<SaveOutput, ApplicationError> {
        Ok(self.save_usecase.execute(input)?)
    }

    pub async fn get_completed_image(
        &self,
        input: GetCompletedInput,
    ) -> Result<GetCompletedOutput, ApplicationError> {
        Ok(self.get_completed_usecase.execute(input)?)
    }

    pub async fn get_preview(
        &self,
        input: GetPreviewInput,
    ) -> Result<GetPreviewOutput, ApplicationError> {
        Ok(self.get_preview_usecase.execute(input)?)
    }

    /// GC タスクから呼ばれる。`now` 時点で期限切れのエントリを全ストアから削除する。
    pub fn cleanup(&self, now: Instant) {
        self.delete_expired_usecase.execute(now);
    }

    pub fn usage_recorder(&self) -> &dyn UsageRecorder {
        self.usage_recorder.as_ref()
    }
}
