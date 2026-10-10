use std::ops::DerefMut;

use crate::session::{id::SessionId, repository::error::SessionRepositoryError, session::Session};

pub trait SessionRepository {
    type Guard<'a>: DerefMut<Target = Session>
    where
        Self: 'a;

    fn save(&self, session: Session);
    fn get<'a>(&'a self, session_id: &SessionId)
    -> Result<Self::Guard<'a>, SessionRepositoryError>;
}
