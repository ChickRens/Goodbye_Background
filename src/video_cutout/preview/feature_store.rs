//! ジョブごとのプレビューの特徴量の置き場所。寿命はジョブと同じ（ジョブの削除・期限切れで消す）。

use std::sync::Arc;

use dashmap::DashMap;

use crate::video_cutout::{job::id::VideoJobId, preview::segmenter::PreviewFeatures};

pub struct PreviewFeatureStore {
    features: DashMap<VideoJobId, Arc<PreviewFeatures>>,
}

impl PreviewFeatureStore {
    pub fn new() -> Self {
        Self {
            features: DashMap::new(),
        }
    }

    /// 同じジョブで登録し直したら丸ごと置き換える。
    pub fn save(&self, job_id: VideoJobId, features: Arc<PreviewFeatures>) {
        self.features.insert(job_id, features);
    }

    pub fn get(&self, job_id: &VideoJobId) -> Option<Arc<PreviewFeatures>> {
        self.features.get(job_id).map(|e| Arc::clone(e.value()))
    }

    pub fn remove(&self, job_id: &VideoJobId) {
        self.features.remove(job_id);
    }

    /// `keep` が `false` を返した ID の特徴量を消す（ジョブが既に無いのに残ったものを掃除するため）。
    pub fn retain_ids(&self, keep: impl Fn(&VideoJobId) -> bool) {
        self.features.retain(|id, _| keep(id));
    }

    #[cfg(test)]
    pub fn len(&self) -> usize {
        self.features.len()
    }
}

#[cfg(test)]
mod preview_feature_store_test {
    use ndarray::Array4;

    use super::*;

    fn features() -> Arc<PreviewFeatures> {
        Arc::new(PreviewFeatures {
            feats0: Array4::zeros((1, 1, 1, 1)),
            feats1: Array4::zeros((1, 1, 1, 1)),
            feats2_no_mem: Array4::zeros((1, 1, 1, 1)),
            frame_width: 4,
            frame_height: 3,
        })
    }

    #[test]
    fn save_get_remove_and_retain() {
        let store = PreviewFeatureStore::new();
        let (a, b) = (VideoJobId::new_random(), VideoJobId::new_random());
        store.save(a, features());
        store.save(b, features());
        assert_eq!(store.get(&a).unwrap().frame_width, 4);
        store.remove(&a);
        assert!(store.get(&a).is_none());
        store.retain_ids(|id| *id != b);
        assert_eq!(store.len(), 0);
    }
}
