use std::time::Instant;

pub trait AppClock: Send + Sync {
    fn now(&self) -> Instant;
}
