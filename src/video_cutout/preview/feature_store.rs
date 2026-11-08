//! ジョブごとのプレビューの特徴量の置き場所。
//!
//! 寿命はジョブと同じ（開始・削除・期限切れで消す）だが、作成は回数を消費しないので無制限に増えうる。
//! メモリを守るため `MAX_PREVIEW_FEATURES` 件を上限にし、溢れたら最も長く使われていないものを捨てる（LRU）。
//! 捨てられたジョブの `/preview` は 409 `PREVIEW_FRAME_NOT_FOUND` になり、フロントがフレームを登録し直す。

use std::{collections::HashMap, sync::Arc, sync::Mutex};

use crate::video_cutout::{job::id::VideoJobId, preview::segmenter::PreviewFeatures};

/// 同時に置いておく特徴量の上限（small は 1 件約 8MB → 最大約 128MB。tiny（1 件約 2MB）の 64 件と同じメモリに収める）。
pub const MAX_PREVIEW_FEATURES: usize = 16;

pub struct PreviewFeatureStore {
    inner: Mutex<Inner>,
}

struct Inner {
    /// 値と、最後に使った順番（大きいほど新しい）。
    entries: HashMap<VideoJobId, (Arc<PreviewFeatures>, u64)>,
    tick: u64,
}

impl Inner {
    fn next_tick(&mut self) -> u64 {
        self.tick += 1;
        self.tick
    }
}

impl PreviewFeatureStore {
    pub fn new() -> Self {
        Self {
            inner: Mutex::new(Inner {
                entries: HashMap::new(),
                tick: 0,
            }),
        }
    }

    /// 同じジョブで登録し直したら丸ごと置き換える。上限を超えるなら最も長く使われていないものを捨てる。
    pub fn save(&self, job_id: VideoJobId, features: Arc<PreviewFeatures>) {
        let mut inner = self.lock();
        if !inner.entries.contains_key(&job_id) && inner.entries.len() >= MAX_PREVIEW_FEATURES {
            let oldest = inner
                .entries
                .iter()
                .min_by_key(|(_, (_, used))| *used)
                .map(|(id, _)| *id);
            if let Some(oldest) = oldest {
                inner.entries.remove(&oldest);
            }
        }
        let tick = inner.next_tick();
        inner.entries.insert(job_id, (features, tick));
    }

    /// 取得したものは「最近使った」扱いになる。
    pub fn get(&self, job_id: &VideoJobId) -> Option<Arc<PreviewFeatures>> {
        let mut inner = self.lock();
        let tick = inner.next_tick();
        inner.entries.get_mut(job_id).map(|(features, used)| {
            *used = tick;
            Arc::clone(features)
        })
    }

    pub fn remove(&self, job_id: &VideoJobId) {
        self.lock().entries.remove(job_id);
    }

    /// `keep` が `false` を返した ID の特徴量を消す（ジョブが既に無いのに残ったものを掃除するため）。
    pub fn retain_ids(&self, keep: impl Fn(&VideoJobId) -> bool) {
        self.lock().entries.retain(|id, _| keep(id));
    }

    #[cfg(test)]
    pub fn len(&self) -> usize {
        self.lock().entries.len()
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Inner> {
        self.inner
            .lock()
            .expect("プレビューの特徴量の Mutex が壊れている")
    }
}

#[cfg(test)]
mod preview_feature_store_test {
    use ndarray::Array4;

    use super::*;

    fn features() -> Arc<PreviewFeatures> {
        Arc::new(PreviewFeatures {
            feats0: Array4::from_elem((1, 1, 1, 1), half::f16::ZERO),
            feats1: Array4::from_elem((1, 1, 1, 1), half::f16::ZERO),
            feats2_no_mem: Array4::from_elem((1, 1, 1, 1), half::f16::ZERO),
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

    #[test]
    fn evicts_least_recently_used_beyond_the_cap() {
        let store = PreviewFeatureStore::new();
        let ids: Vec<VideoJobId> = (0..MAX_PREVIEW_FEATURES)
            .map(|_| VideoJobId::new_random())
            .collect();
        for id in &ids {
            store.save(*id, features());
        }
        assert_eq!(store.len(), MAX_PREVIEW_FEATURES);
        // 一番古い ids[0] を使うと、次に古い ids[1] が捨てられる
        assert!(store.get(&ids[0]).is_some());
        let newcomer = VideoJobId::new_random();
        store.save(newcomer, features());
        assert_eq!(store.len(), MAX_PREVIEW_FEATURES);
        assert!(store.get(&ids[1]).is_none());
        assert!(store.get(&ids[0]).is_some());
        assert!(store.get(&newcomer).is_some());
        // 既にあるジョブの登録し直しでは何も捨てない
        store.save(ids[2], features());
        assert_eq!(store.len(), MAX_PREVIEW_FEATURES);
        assert!(store.get(&ids[3]).is_some());
    }
}
