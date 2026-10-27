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
        usecase::{
            DeleteExpiredEntriesUseCase, ExpiringCompletedImageRepository,
            ExpiringOriginalImageRepository, ExpiringPreviewStorage, ExpiringSegmenterInputStorage,
            ExpiringSessionRepository,
        },
    },
    session::repository::repository_impl::SessionRepositoryInMemory,
};

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
    pub fn new(usage_recorder: Arc<dyn UsageRecorder>) -> Result<Self, ApplicationError> {
        let clock = RealClock::new();

        let session_repo = Arc::new(SessionRepositoryInMemory::new(clock.clone()));
        let image_repo = Arc::new(OriginalImageRepositoryInMemory::new(Arc::new(
            clock.clone(),
        )));
        let loader = FileImageLoader::new();
        let segmenter =
            Sam2Segmenter::new("models").expect("Failed to load SAM2 model during app initialize");
        let editing_session_repo = Arc::new(SAM2EditingSessionRepository::new(clock.clone()));
        let preview_storage = Arc::new(PreviewStorageInMemory::new(clock.clone()));
        let preview_generator = WebPPreviewImageGenerator::new(600);
        let input_storage = Arc::new(SegmenterInputStorageInMemory::new(clock.clone()));
        let input_generator = SAM2InputGenerator::new(1024, 1024);
        let completed_image_generator = WebPCompletedImageGenerator::new();
        let completed_image_repo = Arc::new(CompletedRepositoryInMemory::new(clock.clone()));

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
        let delete_expired_uc = DeleteExpiredEntriesUseCase::new(
            image_repo.clone() as Arc<dyn ExpiringOriginalImageRepository>,
            session_repo.clone() as Arc<dyn ExpiringSessionRepository>,
            completed_image_repo.clone() as Arc<dyn ExpiringCompletedImageRepository>,
            preview_storage.clone() as Arc<dyn ExpiringPreviewStorage>,
            input_storage.clone() as Arc<dyn ExpiringSegmenterInputStorage>,
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

    pub fn usage_recorder(&self) -> &dyn UsageRecorder {
        self.usage_recorder.as_ref()
    }
}
