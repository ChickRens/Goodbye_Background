use crate::{
    editing::{
        prepare_segment::error::PrepareSegmentUseCaseError, redo::error::RedoUseCaseError,
        save::error::SaveUseCaseError, segment::error::SegmentUseCaseError,
        undo::error::UndoUseCaseError,
    },
    image_io::{
        get_completed::error::GetCompletedUseCaseError, get_preview::error::GetPreviewUseCaseError,
        upload::error::UploadUseCaseError,
    },
    parent_error,
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
    }
);
