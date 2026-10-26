use std::sync::Arc;

use crate::session::{id::SessionId, repository::error::SessionRepositoryError, session::Session};

pub trait SessionRepository {
    fn save(&self, session: Session);
    fn get(&self, session_id: &SessionId) -> Result<Arc<Session>, SessionRepositoryError>;
}
