//! 切り抜きワーカー（trait）。ジョブを投入するだけで、結果は通知（`/internal/video-jobs/{id}/events`）で届く。

use async_trait::async_trait;

use super::{error::WorkerError, request::WorkerJobRequest};

#[async_trait]
pub trait CutoutWorker: Send + Sync {
    async fn dispatch(&self, req: &WorkerJobRequest) -> Result<(), WorkerError>;
}
