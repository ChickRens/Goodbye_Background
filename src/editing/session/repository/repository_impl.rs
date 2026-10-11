use std::time::{Duration, Instant};

use dashmap::DashMap;
use dashmap::mapref::one::RefMut;

use crate::editing::segmenter::sam2_data::{SAM2InferenceContext, SAM2StaticContext};
use crate::editing::session::{
    repository::{error::EditingSessionRepositoryError, repository::EditingSessionRepository},
    session::CommonEditingSession,
};
use crate::maintenance::deleting_expired_entries::{
    clock::clock::AppClock,
    entry::{Entry, EntryGuard},
    repository::DeleteExpiredRepository,
};
use crate::session::id::SessionId;

pub struct SAM2EditingSessionRepository<Clock>
where
    Clock: AppClock,
{
    sessions:
        DashMap<SessionId, Entry<CommonEditingSession<SAM2StaticContext, SAM2InferenceContext>>>,
    clock: Clock,
}

impl<Clock> EditingSessionRepository for SAM2EditingSessionRepository<Clock>
where
    Clock: AppClock,
{
    type StaticContext = SAM2StaticContext;
    type InferenceContext = SAM2InferenceContext;
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

    fn save(
        &self,
        session_id: &SessionId,
        editing_session: CommonEditingSession<Self::StaticContext, Self::InferenceContext>,
    ) {
        let entry = Entry::new(editing_session, self.clock.now());
        self.sessions.insert(*session_id, entry);
    }

    fn get<'a>(
        &'a self,
        session_id: &SessionId,
    ) -> Result<Self::Guard<'a>, EditingSessionRepositoryError> {
        self.sessions
            .get_mut(session_id)
            .map(|e| EntryGuard::new(e, self.clock.now()))
            .ok_or(EditingSessionRepositoryError::EditingSessionNotFound)
    }
}

impl<C> DeleteExpiredRepository for SAM2EditingSessionRepository<C>
where
    C: AppClock,
{
    fn delete_expired(&self, now: Instant, ttl: Duration) {
        self.sessions.retain(|_, entry| !entry.is_expired(now, ttl));
    }
}

impl<Clock> SAM2EditingSessionRepository<Clock>
where
    Clock: AppClock,
{
    pub fn new(clock: Clock) -> Self {
        Self {
            sessions: DashMap::new(),
            clock,
        }
    }
}

#[cfg(test)]
mod editing_session_repository_test {
    use std::time::{Duration, Instant};

    use crate::editing::session::inference_context_history::InferenceContextHistory;
    use crate::{
        editing::segmenter::sam2_data::{SAM2InferenceContext, SAM2StaticContext},
        editing::session::{
            repository::{
                error::EditingSessionRepositoryError, repository::EditingSessionRepository,
                repository_impl::SAM2EditingSessionRepository,
            },
            session::CommonEditingSession,
        },
        maintenance::deleting_expired_entries::{
            clock::clock_impl::FakeClock, repository::DeleteExpiredRepository,
        },
        point::point_history::PointHistory,
        session::id::SessionId,
    };

    type EditingSession = CommonEditingSession<SAM2StaticContext, SAM2InferenceContext>;

    fn make_session(max_undo: usize) -> EditingSession {
        let static_context = SAM2StaticContext::default();
        let inference_context = SAM2InferenceContext::default();

        CommonEditingSession::new(
            PointHistory::new(max_undo),
            static_context,
            InferenceContextHistory::new(max_undo, inference_context),
            2.0,
        )
    }

    #[test]
    fn save_and_get_returns_the_saved_session() {
        let repo = SAM2EditingSessionRepository::new(FakeClock::new(Instant::now()));
        let session = make_session(10);
        let session_id = SessionId::new();

        repo.save(&session_id, session.clone());

        let fetched = repo.get(&session_id);
        assert!(fetched.is_ok());
    }

    #[test]
    fn get_missing_session_returns_not_found() {
        let repo = SAM2EditingSessionRepository::new(FakeClock::new(Instant::now()));

        let result = repo.get(&SessionId::new());

        assert!(matches!(
            result,
            Err(EditingSessionRepositoryError::EditingSessionNotFound)
        ));
    }

    #[test]
    fn delete_expired_removes_old_sessions() {
        let base = Instant::now();
        let repo = SAM2EditingSessionRepository::new(FakeClock::new(base));
        let session = make_session(10);
        let session_id = SessionId::new();

        repo.save(&session_id, session.clone());
        repo.delete_expired(base + Duration::from_secs(30), Duration::from_secs(10));

        assert!(repo.get(&session_id).is_err());
    }
}
