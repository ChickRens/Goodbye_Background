use std::sync::Arc;

use crate::{
    editing::session::{
        repository::error::EditingSessionRepositoryError, session::CommonEditingSession,
    },
    session::id::SessionId,
};

pub trait EditingSessionRepository {
    type StaticContext;
    type InferenceContext;

    fn save(
        &self,
        session_id: &SessionId,
        editing_session: CommonEditingSession<Self::StaticContext, Self::InferenceContext>,
    );
    fn get(
        &self,
        session_id: &SessionId,
    ) -> Result<
        Arc<CommonEditingSession<Self::StaticContext, Self::InferenceContext>>,
        EditingSessionRepositoryError,
    >;
}
