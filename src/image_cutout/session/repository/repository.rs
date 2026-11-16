//! セッション保存先のトレイト。

use std::sync::Arc;

use crate::image_cutout::session::{id::SessionId, repository::error::SessionRepositoryError, session::Session};

pub trait SessionRepository: Send + Sync {
    fn save(&self, session: Session);
    fn get(&self, session_id: &SessionId) -> Result<Arc<Session>, SessionRepositoryError>;
}
