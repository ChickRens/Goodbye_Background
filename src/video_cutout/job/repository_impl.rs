//! インメモリ実装。期限は `VideoJob::deadline_ms()` で判断し、TtlStore の最終アクセス基準は使わない（仕様 §4.4）。

use std::sync::Arc;

use dashmap::DashMap;

use crate::video_cutout::job::{
    id::VideoJobId, repository::VideoJobRepository, video_job::VideoJob,
};

pub struct VideoJobRepositoryInMemory {
    jobs: DashMap<VideoJobId, Arc<VideoJob>>,
}

impl VideoJobRepositoryInMemory {
    pub fn new() -> Self {
        Self { jobs: DashMap::new() }
    }
}

impl VideoJobRepository for VideoJobRepositoryInMemory {
    fn get(&self, id: &VideoJobId) -> Option<Arc<VideoJob>> {
        self.jobs.get(id).map(|e| Arc::clone(e.value()))
    }

    fn save(&self, job: VideoJob) {
        self.jobs.insert(job.id, Arc::new(job));
    }

    fn remove(&self, id: &VideoJobId) {
        self.jobs.remove(id);
    }

    fn list_expired(&self, now_ms: u64) -> Vec<Arc<VideoJob>> {
        self.jobs
            .iter()
            .filter(|e| e.value().deadline_ms() < now_ms)
            .map(|e| Arc::clone(e.value()))
            .collect()
    }
}

#[cfg(test)]
mod video_job_repository_test {
    use super::*;
    use crate::video_cutout::job::video_job::job_created_at;

    #[test]
    fn save_get_remove() {
        let r = VideoJobRepositoryInMemory::new();
        let j = job_created_at(0);
        r.save(j.clone());
        assert_eq!(*r.get(&j.id).unwrap(), j);
        r.remove(&j.id);
        assert!(r.get(&j.id).is_none());
    }

    #[test]
    fn list_expired_uses_deadline() {
        let r = VideoJobRepositoryInMemory::new();
        r.save(job_created_at(0));
        r.save(job_created_at(10 * 60_000));
        assert_eq!(r.list_expired(30 * 60_000 + 1).len(), 1);
    }
}
