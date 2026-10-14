use std::time::Instant;

use crate::{
    analytics::usage_tracking::recorder::PostgresUsageRecorder,
    application::error::ApplicationError,
    config::{MAX_HISTORY, TTL_SEC},
    editing::{
        prepare_segment::{input::PrepareSegmentInput, usecase::PrepareSegmentUseCase},
        redo::{input::RedoInput, output::RedoOutput, usecase::RedoUseCase},
        save::{input::SaveInput, output::SaveOutput, usecase::SaveUseCase},
        segment::{input::SegmentInput, output::SegmentOutput, usecase::SegmentUseCase},
        segmenter::{
            prepare_service::SharedPrepareSegmentService, segment_service::SharedSegmentService,
            shared_segmenter_sam2::SharedSAM2Segmenter,
        },
        session::repository::shared_repository_impl::SharedEditingSessionRepository,
        undo::{input::UndoInput, output::UndoOutput, usecase::UndoUseCase},
    },
    image::{
        completed::{
            completed_service::SharedCompletedService,
            generator::generator::SharedWebPCompletedImageGenerator,
            storage::storage_impl::SharedCompletedImageRepository,
        },
        original::original_repository::shared_repository_impl::SharedOriginalImageRepository,
        preview::{
            generator::shared_generator_impl::SharedPreviewGenerator,
            service::SharedPreviewService, storage::storage_impl::SharedPreviewStorage,
        },
        segmenter_input::{
            generator::shared_generator_impl::SharedSAM2InputGenerator,
            storage::shared_storage_impl::SharedSegmenterInputStorage,
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
        clock::clock_impl::RealClock, usecase::DeleteExpiredEntriesUseCase,
    },
    session::repository::shared_repository_impl::SharedSessionRepository,
};

pub struct App {
    upload_usecase: UploadUseCase<
        SharedSessionRepository<RealClock>,
        SharedOriginalImageRepository<RealClock>,
        FileImageLoader,
        SharedPreviewService<SharedPreviewGenerator, SharedPreviewStorage<RealClock>>,
    >,
    prepare_segment_usecase: PrepareSegmentUseCase<
        SharedPrepareSegmentService<
            SharedSAM2Segmenter,
            SharedOriginalImageRepository<RealClock>,
            SharedEditingSessionRepository<RealClock>,
            SharedSAM2InputGenerator,
            SharedSegmenterInputStorage<RealClock>,
        >,
    >,
    segment_usecase: SegmentUseCase<
        SharedSegmentService<
            SharedSessionRepository<RealClock>,
            SharedSAM2Segmenter,
            SharedOriginalImageRepository<RealClock>,
            SharedEditingSessionRepository<RealClock>,
            SharedSegmenterInputStorage<RealClock>,
        >,
        SharedPreviewService<SharedPreviewGenerator, SharedPreviewStorage<RealClock>>,
    >,
    undo_usecase: UndoUseCase<
        SharedSegmentService<
            SharedSessionRepository<RealClock>,
            SharedSAM2Segmenter,
            SharedOriginalImageRepository<RealClock>,
            SharedEditingSessionRepository<RealClock>,
            SharedSegmenterInputStorage<RealClock>,
        >,
        SharedPreviewService<SharedPreviewGenerator, SharedPreviewStorage<RealClock>>,
    >,
    redo_usecase: RedoUseCase<
        SharedPreviewService<SharedPreviewGenerator, SharedPreviewStorage<RealClock>>,
        SharedSegmentService<
            SharedSessionRepository<RealClock>,
            SharedSAM2Segmenter,
            SharedOriginalImageRepository<RealClock>,
            SharedEditingSessionRepository<RealClock>,
            SharedSegmenterInputStorage<RealClock>,
        >,
    >,
    save_usecase: SaveUseCase<
        SharedSegmentService<
            SharedSessionRepository<RealClock>,
            SharedSAM2Segmenter,
            SharedOriginalImageRepository<RealClock>,
            SharedEditingSessionRepository<RealClock>,
            SharedSegmenterInputStorage<RealClock>,
        >,
        SharedCompletedService<
            SharedWebPCompletedImageGenerator,
            SharedCompletedImageRepository<RealClock>,
        >,
    >,
    get_completed_usecase: GetCompletedUseCase<
        SharedCompletedService<
            SharedWebPCompletedImageGenerator,
            SharedCompletedImageRepository<RealClock>,
        >,
    >,
    get_preview_usecase: GetPreviewUseCase<
        SharedPreviewService<SharedPreviewGenerator, SharedPreviewStorage<RealClock>>,
    >,
    delete_expired_usecase: DeleteExpiredEntriesUseCase<
        SharedOriginalImageRepository<RealClock>,
        SharedSessionRepository<RealClock>,
        SharedCompletedImageRepository<RealClock>,
        SharedPreviewStorage<RealClock>,
        SharedSegmenterInputStorage<RealClock>,
    >,
    usage_recorder: PostgresUsageRecorder,
}

impl App {
    pub fn new(pool: sqlx::Pool<sqlx::Postgres>) -> Result<Self, ApplicationError> {
        let clock = RealClock::new();

        let session_repo = SharedSessionRepository::new(clock.clone());
        let image_repo = SharedOriginalImageRepository::new(clock.clone());
        let loader = FileImageLoader::new();
        let segmenter = SharedSAM2Segmenter::new("models")
            .expect("Failed to load SAM2 model during app initialize");
        let editing_session_repo = SharedEditingSessionRepository::new(clock.clone());
        let preview_storage = SharedPreviewStorage::new(clock.clone());
        let preview_generator = SharedPreviewGenerator::new(600);
        let input_storage = SharedSegmenterInputStorage::new(clock.clone());
        let input_generator = SharedSAM2InputGenerator::new(1024, 1024);
        let completed_image_generator = SharedWebPCompletedImageGenerator::new();
        let completed_image_repo = SharedCompletedImageRepository::new(clock.clone());

        let preview_service = SharedPreviewService::new(preview_generator, preview_storage.clone());
        let prepare_service = SharedPrepareSegmentService::new(
            segmenter.clone(),
            image_repo.clone(),
            editing_session_repo.clone(),
            input_generator,
            input_storage.clone(),
            MAX_HISTORY,
        );
        let segment_service = SharedSegmentService::new(
            session_repo.clone(),
            segmenter.clone(),
            image_repo.clone(),
            editing_session_repo.clone(),
            input_storage.clone(),
        );
        let completed_service =
            SharedCompletedService::new(completed_image_generator, completed_image_repo.clone());

        let upload_uc = UploadUseCase::new(
            session_repo.clone(),
            image_repo.clone(),
            loader,
            preview_service.clone(),
        );
        let prepare_segment_uc = PrepareSegmentUseCase::new(prepare_service);
        let segment_uc = SegmentUseCase::new(segment_service.clone(), preview_service.clone());
        let undo_uc = UndoUseCase::new(segment_service.clone(), preview_service.clone());
        let redo_uc = RedoUseCase::new(segment_service.clone(), preview_service.clone());
        let save_uc = SaveUseCase::new(segment_service.clone(), completed_service.clone());
        let get_completed_uc = GetCompletedUseCase::new(completed_service.clone());
        let get_image_uc = GetPreviewUseCase::new(preview_service.clone());
        let delete_expired_uc = DeleteExpiredEntriesUseCase::new(
            image_repo.clone(),
            session_repo.clone(),
            completed_image_repo.clone(),
            preview_storage.clone(),
            input_storage.clone(),
            TTL_SEC,
        );

        let usage_recorder = PostgresUsageRecorder::new(pool);

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

    pub fn upload(&self, input: UploadInput) -> Result<UploadOutput, ApplicationError> {
        let output = self.upload_usecase.execute(input)?;
        let preview_to_original_point_scale = output.scale();
        let (session_id, image_id) = output.session_id_and_image_id();

        let input = PrepareSegmentInput::new(image_id, session_id, preview_to_original_point_scale);
        self.prepare_segment_usecase.execute(input)?;
        Ok(output)
    }

    pub fn segment(&self, input: SegmentInput) -> Result<SegmentOutput, ApplicationError> {
        Ok(self.segment_usecase.execute(input)?)
    }

    pub fn undo(&self, input: UndoInput) -> Result<UndoOutput, ApplicationError> {
        Ok(self.undo_usecase.execute(input)?)
    }

    pub fn redo(&self, input: RedoInput) -> Result<RedoOutput, ApplicationError> {
        Ok(self.redo_usecase.execute(input)?)
    }

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

    pub fn cleanup(&self, now: Instant) {
        self.delete_expired_usecase.execute(now);
    }

    pub fn usage_recorder(&self) -> &PostgresUsageRecorder {
        &self.usage_recorder
    }
}
