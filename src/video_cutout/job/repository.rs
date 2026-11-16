//! 動画ジョブの保存先（trait）。値は不変で、丸ごと置き換えて保存する。

use std::sync::Arc;

use crate::video_cutout::job::{id::VideoJobId, video_job::VideoJob};

pub trait VideoJobRepository: Send + Sync {
    fn get(&self, id: &VideoJobId) -> Option<Arc<VideoJob>>;
    fn save(&self, job: VideoJob);
    fn remove(&self, id: &VideoJobId);
    /// `deadline_ms()` が `now_ms` より前のジョブを返す。
    fn list_expired(&self, now_ms: u64) -> Vec<Arc<VideoJob>>;
}
