use crate::session::id::SessionId;

pub struct SaveInput {
    session_id: SessionId,
}

impl SaveInput {
    pub fn new(session_id: SessionId) -> Self {
        Self { session_id }
    }

    pub fn session_id(&self) -> SessionId {
        self.session_id
    }
}
