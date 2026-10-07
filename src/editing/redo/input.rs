use crate::domain::value_object::session_id::session_id::SessionId;

pub struct RedoInput {
    session_id: SessionId,
}

impl RedoInput {
    pub fn new(session_id: SessionId) -> Self {
        Self { session_id }
    }

    pub fn session_id(&self) -> SessionId {
        self.session_id
    }
}
