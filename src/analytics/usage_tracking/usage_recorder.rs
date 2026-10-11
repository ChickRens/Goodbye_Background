use crate::application::types::usage::event::UsageEvent;

pub trait UsageRecorder {
    async fn record(&self, event: UsageEvent);
}
