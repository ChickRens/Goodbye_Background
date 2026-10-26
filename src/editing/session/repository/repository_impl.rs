use std::{
    sync::Arc,
    time::{Duration, Instant},
};

use crate::editing::segmenter::sam2_data::{SAM2InferenceContext, SAM2StaticContext};
use crate::editing::session::{
    repository::{error::EditingSessionRepositoryError, repository::EditingSessionRepository},
    session::CommonEditingSession,
};
use crate::maintenance::deleting_expired_entries::{
    clock::clock::AppClock, repository::DeleteExpiredRepository,
};
use crate::maintenance::ttl_store::TtlStore;
use crate::session::id::SessionId;

pub struct SAM2EditingSessionRepository {
    sessions: TtlStore<SessionId, CommonEditingSession<SAM2StaticContext, SAM2InferenceContext>>,
}

impl EditingSessionRepository for SAM2EditingSessionRepository {
    type StaticContext = SAM2StaticContext;
    type InferenceContext = SAM2InferenceContext;

    fn save(
        &self,
        session_id: &SessionId,
        editing_session: CommonEditingSession<Self::StaticContext, Self::InferenceContext>,
    ) {
        self.sessions.insert(*session_id, editing_session);
    }

    fn get(
        &self,
        session_id: &SessionId,
    ) -> Result<
        Arc<CommonEditingSession<Self::StaticContext, Self::InferenceContext>>,
        EditingSessionRepositoryError,
    > {
        self.sessions
            .get(*session_id)
            .ok_or(EditingSessionRepositoryError::EditingSessionNotFound)
    }
}

impl DeleteExpiredRepository for SAM2EditingSessionRepository {
    fn delete_expired(&self, now: Instant, ttl: Duration) {
        self.sessions.delete_expired(now, ttl);
    }
}

impl SAM2EditingSessionRepository {
    pub fn new(clock: impl AppClock + 'static) -> Self {
        Self {
            sessions: TtlStore::new(Arc::new(clock)),
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
