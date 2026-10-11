use std::ops::DerefMut;

use crate::{
    application::{
        interface::editing_session_repository::error::EditingSessionRepositoryError,
        types::editing_session::session::CommonEditingSession,
    },
    domain::value_object::session_id::session_id::SessionId,
};

pub trait EditingSessionRepository {
    type StaticContext;
    type InferenceContext;
    type Guard<'a>: DerefMut<
        Target = CommonEditingSession<Self::StaticContext, Self::InferenceContext>,
    >
    where
        Self: 'a;

    fn save(
        &self,
        session_id: &SessionId,
        editing_session: CommonEditingSession<Self::StaticContext, Self::InferenceContext>,
    );
    fn get<'a>(
        &'a self,
        session_id: &SessionId,
    ) -> Result<Self::Guard<'a>, EditingSessionRepositoryError>;
}
