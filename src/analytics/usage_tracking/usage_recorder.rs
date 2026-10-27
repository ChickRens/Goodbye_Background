use crate::analytics::usage_tracking::event::UsageEvent;

pub trait UsageRecorder: Send + Sync {
    fn record(&self, event: UsageEvent);
}
