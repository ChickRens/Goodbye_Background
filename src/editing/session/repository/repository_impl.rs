use std::time::{Duration, Instant};

use dashmap::DashMap;
use dashmap::mapref::one::RefMut;

use crate::application::interface::clock::AppClock;
use crate::application::interface::delete_expired_repository::DeleteExpiredRepository;
use crate::application::interface::editing_session_repository::error::EditingSessionRepositoryError;
use crate::application::interface::editing_session_repository::repository::EditingSessionRepository;
use crate::application::types::editing_session::session::CommonEditingSession;
use crate::application::types::entry::{Entry, EntryGuard};
use crate::domain::value_object::session_id::session_id::SessionId;
use crate::infrastructure::segmenter::sam2_data::{SAM2InferenceContext, SAM2StaticContext};

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

    use crate::{
        application::{
            interface::{
                delete_expired_repository::DeleteExpiredRepository,
                editing_session_repository::{
                    error::EditingSessionRepositoryError, repository::EditingSessionRepository,
                },
            },
            types::{
                editing_session::session::CommonEditingSession,
                inference_context_history::InferenceContextHistory, point_history::PointHistory,
            },
        },
        domain::value_object::session_id::session_id::SessionId,
        infrastructure::{
            clock::FakeClock,
            repository::editing_session_repository::SAM2EditingSessionRepository,
            segmenter::sam2_data::{SAM2InferenceContext, SAM2StaticContext},
        },
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
