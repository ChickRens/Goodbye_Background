use uuid::Uuid;

pub struct UsageId {
    value: Uuid,
}

impl UsageId {
    pub fn new(value: Uuid) -> Self {
        Self { value }
    }

    pub fn value(&self) -> Uuid {
        self.value
    }
}
