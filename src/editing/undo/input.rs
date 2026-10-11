use crate::session::id::SessionId;

pub struct UndoInput {
    session_id: SessionId,
}

impl UndoInput {
    pub fn new(session_id: SessionId) -> Self {
        Self { session_id }
    }

    pub fn session_id(&self) -> SessionId {
        self.session_id
    }
}
