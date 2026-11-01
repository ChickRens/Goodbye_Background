use uuid::Uuid;

#[derive(Debug, PartialEq, Eq, Clone, Copy, Hash)]
pub struct UserId {
    id: Uuid,
}

impl UserId {
    pub fn new() -> Self {
        Self { id: Uuid::new_v4() }
    }

    pub fn from_uuid(id: Uuid) -> Self {
        Self { id }
    }

    pub fn value(&self) -> &Uuid {
        &self.id
    }
}
