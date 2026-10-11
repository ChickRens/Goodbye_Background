use std::time::Duration;

use crate::application::types::usage::{
    id::UsageId, operation::UsageOperation, status::UsageStatus,
};

pub struct UsageEvent {
    id: Option<UsageId>,
    operation: UsageOperation,
    status: UsageStatus,
    processing_time: Duration,
}

impl UsageEvent {
    pub fn new(
        id: Option<UsageId>,
        operation: UsageOperation,
        status: UsageStatus,
        processing_time: Duration,
    ) -> Self {
        Self {
            id,
            operation,
            status,
            processing_time,
        }
    }

    pub fn operation(&self) -> &UsageOperation {
        &self.operation
    }

    pub fn status(&self) -> &UsageStatus {
        &self.status
    }

    pub fn processing_time(&self) -> Duration {
        self.processing_time
    }

    pub fn id(&self) -> Option<&UsageId> {
        self.id.as_ref()
    }
}
