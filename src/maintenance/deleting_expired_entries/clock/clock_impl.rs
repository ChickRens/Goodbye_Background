use std::time::Instant;

use crate::application::interface::clock::AppClock;

#[derive(Debug, Clone)]
pub struct RealClock;

impl AppClock for RealClock {
    fn now(&self) -> Instant {
        Instant::now()
    }
}

impl RealClock {
    pub fn new() -> Self {
        Self
    }
}

#[derive(Debug, Clone)]
pub struct FakeClock {
    time: Instant,
}

impl AppClock for FakeClock {
    fn now(&self) -> Instant {
        self.time
    }
}

impl FakeClock {
    pub fn new(time: Instant) -> Self {
        Self { time }
    }
}
