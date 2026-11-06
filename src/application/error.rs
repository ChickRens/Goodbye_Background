//! 全ユースケースのエラーを1つにまとめた `ApplicationError`。
//!
//! `App` の各メソッドはこの型を返し、ハンドラ側で `AppError` に `?` で変換される。
//! 新しいユースケースを追加したら、ここにも variant を足すこと（足さないと `?` が通らない）。

use crate::{
    image_cutout::editing::{
        prepare_segment::error::PrepareSegmentUseCaseError, redo::error::RedoUseCaseError,
        save::error::SaveUseCaseError, segment::error::SegmentUseCaseError,
        undo::error::UndoUseCaseError,
    },
    image_cutout::image_io::{
        get_completed::error::GetCompletedUseCaseError, get_preview::error::GetPreviewUseCaseError,
        upload::error::UploadUseCaseError,
    },
    parent_error,
    video_cutout::{
        create_job::error::CreateVideoJobError, error::VideoCutoutError,
        get_job::error::GetVideoJobError,
        report_event::error::ReportVideoJobEventError, start_job::error::StartVideoJobError,
    },
};

parent_error!(
    pub enum ApplicationError {
        GetPreviewUseCase(GetPreviewUseCaseError),
        RedoUseCase(RedoUseCaseError),
        SegmentUseCase(SegmentUseCaseError),
        UndoUseCase(UndoUseCaseError),
        UploadUseCase(UploadUseCaseError),
        SaveUseCase(SaveUseCaseError),
        GetCompletedUseCase(GetCompletedUseCaseError),
        PrepareSegmentUseCase(PrepareSegmentUseCaseError),
        CreateVideoJobUseCase(CreateVideoJobError),
        StartVideoJobUseCase(StartVideoJobError),
        GetVideoJobUseCase(GetVideoJobError),
        ReportVideoJobEventUseCase(ReportVideoJobEventError),
        VideoCutout(VideoCutoutError),
    }
);
