use std::sync::Arc;
use std::time::{Duration, Instant};

use dashmap::mapref::one::RefMut;

use crate::editing::segmenter::sam2_data::{SAM2InferenceContext, SAM2StaticContext};
use crate::editing::session::{
    repository::{
        error::EditingSessionRepositoryError, repository::EditingSessionRepository,
        repository_impl::SAM2EditingSessionRepository,
    },
    session::CommonEditingSession,
};
use crate::maintenance::deleting_expired_entries::{
    clock::clock::AppClock,
    entry::{Entry, EntryGuard},
    repository::DeleteExpiredRepository,
};
use crate::session::id::SessionId;

#[derive(Clone)]
pub struct SharedEditingSessionRepository<Clock>
where
    Clock: AppClock,
{
    sessions: Arc<SAM2EditingSessionRepository<Clock>>,
}

impl<Clock> SharedEditingSessionRepository<Clock>
where
    Clock: AppClock,
{
    pub fn new(clock: Clock) -> Self {
        Self {
            sessions: Arc::new(SAM2EditingSessionRepository::new(clock)),
        }
    }
}

impl<Clock> EditingSessionRepository for SharedEditingSessionRepository<Clock>
where
    Clock: AppClock,
{
    type InferenceContext = SAM2InferenceContext;
    type StaticContext = SAM2StaticContext;
    type Guard<'a>
        = EntryGuard<
        RefMut<
            'a,
            SessionId,
            Entry<CommonEditingSession<Self::StaticContext, Self::InferenceContext>>,
        >,
        CommonEditingSession<Self::StaticContext, Self::InferenceContext>,
    >
    where
        Self: 'a;

    fn get<'a>(
        &'a self,
        session_id: &SessionId,
    ) -> Result<Self::Guard<'a>, EditingSessionRepositoryError> {
        self.sessions.get(session_id)
    }

    fn save(
        &self,
        session_id: &SessionId,
        editing_session: CommonEditingSession<Self::StaticContext, Self::InferenceContext>,
    ) {
        self.sessions.save(session_id, editing_session);
    }
}

impl<Clock> DeleteExpiredRepository for SharedEditingSessionRepository<Clock>
where
    Clock: AppClock,
{
    fn delete_expired(&self, now: Instant, ttl: Duration) {
        self.sessions.delete_expired(now, ttl);
    }
}
