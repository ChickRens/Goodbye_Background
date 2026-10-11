use crate::analytics::usage_tracking::event::UsageEvent;

pub trait UsageRecorder {
    async fn record(&self, event: UsageEvent);
}
