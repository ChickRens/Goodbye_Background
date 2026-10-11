use std::sync::Arc;
use std::time::{Duration, Instant};

use dashmap::mapref::one::RefMut;

use crate::application::interface::{
    clock::AppClock, delete_expired_repository::DeleteExpiredRepository,
};
use crate::application::types::entry::{Entry, EntryGuard};
use crate::session::id::SessionId;
use crate::session::repository::error::SessionRepositoryError;
use crate::session::repository::implement::repository_impl::SessionRepositoryInMemory;
use crate::session::repository::repository::SessionRepository;
use crate::session::session::Session;

#[derive(Clone)]
pub struct SharedSessionRepository<Clock>
where
    Clock: AppClock,
{
    sessions: Arc<SessionRepositoryInMemory<Clock>>,
}

impl<Clock> SessionRepository for SharedSessionRepository<Clock>
where
    Clock: AppClock,
{
    type Guard<'a>
        = EntryGuard<RefMut<'a, SessionId, Entry<Session>>, Session>
    where
        Self: 'a;

    fn save(&self, session: Session) {
        self.sessions.save(session);
    }

    fn get<'a>(
        &'a self,
        session_id: &SessionId,
    ) -> Result<Self::Guard<'a>, SessionRepositoryError> {
        self.sessions.get(session_id)
    }
}

impl<Clock> DeleteExpiredRepository for SharedSessionRepository<Clock>
where
    Clock: AppClock,
{
    fn delete_expired(&self, now: Instant, ttl: Duration) {
        self.sessions.delete_expired(now, ttl);
    }
}

impl<Clock> SharedSessionRepository<Clock>
where
    Clock: AppClock,
{
    pub fn new(clock: Clock) -> Self {
        Self {
            sessions: Arc::new(SessionRepositoryInMemory::new(clock)),
        }
    }
}
