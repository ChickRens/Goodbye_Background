//! ジョブ単位のロック。同じ ID には同じ Mutex を返し、状態の取得→変更→保存を直列化する。

use std::sync::Arc;

use dashmap::DashMap;
use tokio::sync::Mutex;

use crate::video_cutout::job::id::VideoJobId;

pub struct VideoJobLocks {
    locks: DashMap<VideoJobId, Arc<Mutex<()>>>,
}

impl VideoJobLocks {
    pub fn new() -> Self {
        Self { locks: DashMap::new() }
    }

    pub fn lock_for(&self, id: &VideoJobId) -> Arc<Mutex<()>> {
        Arc::clone(self.locks.entry(*id).or_insert_with(|| Arc::new(Mutex::new(()))).value())
    }

    pub fn remove(&self, id: &VideoJobId) {
        self.locks.remove(id);
    }
}

#[cfg(test)]
mod video_job_locks_test {
    use super::*;

    #[tokio::test]
    async fn same_id_shares_lock() {
        let l = VideoJobLocks::new();
        let id = VideoJobId::new_random();
        let _g = l.lock_for(&id).lock_owned().await;
        assert!(l.lock_for(&id).try_lock().is_err());
    }

    #[tokio::test]
    async fn different_ids_do_not_block_and_remove_resets() {
        let l = VideoJobLocks::new();
        let (a, b) = (VideoJobId::new_random(), VideoJobId::new_random());
        let _g = l.lock_for(&a).lock_owned().await;
        assert!(l.lock_for(&b).try_lock().is_ok());
        l.remove(&a);
        assert!(l.lock_for(&a).try_lock().is_ok());
    }
}
