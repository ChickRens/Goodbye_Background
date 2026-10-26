use std::{
    sync::Arc,
    time::{Duration, Instant},
};

use crate::{
    maintenance::deleting_expired_entries::{
        clock::clock::AppClock, repository::DeleteExpiredRepository,
    },
    maintenance::ttl_store::TtlStore,
    session::{
        id::SessionId,
        repository::{error::SessionRepositoryError, repository::SessionRepository},
        session::Session,
    },
};

pub struct SessionRepositoryInMemory {
    sessions: TtlStore<SessionId, Session>,
}

impl SessionRepository for SessionRepositoryInMemory {
    fn save(&self, session: Session) {
        self.sessions.insert(*session.session_id(), session);
    }

    fn get(&self, session_id: &SessionId) -> Result<Arc<Session>, SessionRepositoryError> {
        self.sessions
            .get(*session_id)
            .ok_or(SessionRepositoryError::SessionNotFound)
    }
}

impl DeleteExpiredRepository for SessionRepositoryInMemory {
    fn delete_expired(&self, now: Instant, ttl: Duration) {
        self.sessions.delete_expired(now, ttl);
    }
}

impl SessionRepositoryInMemory {
    pub fn new(clock: impl AppClock + 'static) -> Self {
        Self {
            sessions: TtlStore::new(Arc::new(clock)),
        }
    }
}

#[cfg(test)]
mod session_repository_test {
    use std::time::{Duration, Instant};

    use super::SessionRepositoryInMemory;
    use crate::{
        image::image::id::ImageId,
        maintenance::deleting_expired_entries::{
            clock::clock_impl::FakeClock, repository::DeleteExpiredRepository,
        },
        session::{
            id::SessionId,
            repository::{error::SessionRepositoryError, repository::SessionRepository},
            session::Session,
        },
    };

    fn make_session() -> Session {
        let session_id = SessionId::new();
        let image_id = ImageId::new();
        Session::new(session_id, image_id)
    }

    #[test]
    fn save_and_get_returns_the_saved_session() {
        let now = Instant::now();
        let repo = SessionRepositoryInMemory::new(FakeClock::new(now));
        let session = make_session();
        let session_id = session.session_id();

        repo.save(session.clone());

        let fetched = repo.get(&session_id).unwrap();
        assert_eq!(fetched.session_id(), session.session_id());
    }

    #[test]
    fn get_missing_session_returns_not_found() {
        let repo = SessionRepositoryInMemory::new(FakeClock::new(Instant::now()));

        let result = repo.get(&SessionId::new());

        assert!(matches!(
            result,
            Err(SessionRepositoryError::SessionNotFound)
        ));
    }

    #[test]
    fn delete_expired_removes_old_sessions() {
        let base = Instant::now();
        let repo = SessionRepositoryInMemory::new(FakeClock::new(base));
        let session = make_session();
        let session_id = session.session_id();

        repo.save(session.clone());
        repo.delete_expired(base + Duration::from_secs(30), Duration::from_secs(10));

        assert!(repo.get(&session_id).is_err());
    }
}
