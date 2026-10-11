use std::time::Instant;

pub trait AppClock {
    fn now(&self) -> Instant;
}
