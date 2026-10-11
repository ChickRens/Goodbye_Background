use std::time::{Duration, Instant};

use dashmap::{DashMap, mapref::one::RefMut};

use crate::{
    maintenance::deleting_expired_entries::{
        clock::clock::AppClock,
        entry::{Entry, EntryGuard},
        repository::DeleteExpiredRepository,
    },
    session::{
        id::SessionId,
        repository::{error::SessionRepositoryError, repository::SessionRepository},
        session::Session,
    },
};

pub struct SessionRepositoryInMemory<Clock>
where
    Clock: AppClock,
{
    sessions: DashMap<SessionId, Entry<Session>>,
    clock: Clock,
}

impl<Clock> SessionRepository for SessionRepositoryInMemory<Clock>
where
    Clock: AppClock,
{
    type Guard<'a>
        = EntryGuard<RefMut<'a, SessionId, Entry<Session>>, Session>
    where
        Self: 'a;

    fn save(&self, session: Session) {
        let session_id = session.session_id().clone();
        let entry = Entry::new(session, self.clock.now());
        self.sessions.insert(session_id, entry);
    }

    fn get<'a>(
        &'a self,
        session_id: &SessionId,
    ) -> Result<Self::Guard<'a>, SessionRepositoryError> {
        self.sessions
            .get_mut(session_id)
            .map(|entry| EntryGuard::new(entry, self.clock.now()))
            .ok_or(SessionRepositoryError::SessionNotFound)
    }
}

impl<Clock> DeleteExpiredRepository for SessionRepositoryInMemory<Clock>
where
    Clock: AppClock,
{
    fn delete_expired(&self, now: Instant, ttl: Duration) {
        self.sessions.retain(|_, entry| !entry.is_expired(now, ttl));
    }
}

impl<Clock> SessionRepositoryInMemory<Clock>
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
