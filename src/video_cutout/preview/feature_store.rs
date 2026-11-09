//! ジョブと時刻（点を打つフレーム）ごとのプレビューの特徴量の置き場所。1 ジョブ最大 `MAX_PROMPTS` 枚。
//!
//! 寿命はジョブと同じ（開始・削除・期限切れで消す）だが、作成は回数を消費しないので無制限に増えうる。
//! メモリを守るため全体で `MAX_PREVIEW_FEATURES` 枚を上限にし、溢れたら最も長く使われていない 1 枚を捨てる（LRU）。
//! 捨てられたジョブの `/preview` は 409 `PREVIEW_FRAME_NOT_FOUND` になり、フロントがフレームを登録し直す。

use std::{collections::HashMap, sync::Arc, sync::Mutex};

use crate::video_cutout::{
    job::{id::VideoJobId, prompt::MAX_PROMPTS},
    preview::segmenter::PreviewFeatures,
};

/// 同時に置いておく特徴量の上限（small は 1 件約 8MB → 最大約 128MB。tiny（1 件約 2MB）の 64 件と同じメモリに収める）。
pub const MAX_PREVIEW_FEATURES: usize = 16;

pub struct PreviewFeatureStore {
    inner: Mutex<Inner>,
}

/// 特徴量のキー: ジョブと、そのフレームの時刻（元の動画のミリ秒）。
type Key = (VideoJobId, u32);

struct Inner {
    /// 値と、最後に使った順番（大きいほど新しい）。
    entries: HashMap<Key, (Arc<PreviewFeatures>, u64)>,
    tick: u64,
}

impl Inner {
    fn next_tick(&mut self) -> u64 {
        self.tick += 1;
        self.tick
    }

    /// `which` に当てはまるもののうち、最も長く使われていない 1 枚を捨てる。
    fn evict_oldest(&mut self, which: impl Fn(&Key) -> bool) {
        let oldest = self
            .entries
            .iter()
            .filter(|(k, _)| which(k))
            .min_by_key(|(_, (_, used))| *used)
            .map(|(k, _)| *k);
        if let Some(oldest) = oldest {
            self.entries.remove(&oldest);
        }
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
    /// その時刻のフレームの特徴量を置く（同じ時刻は置き換え）。新しい時刻で、
    /// - そのジョブがすでに `MAX_PROMPTS` 枚持っていたら、そのジョブの最も長く使われていない 1 枚を捨てる
    ///   （画面でフレームを消しても特徴量は残るので、拒否すると「消して別の時刻を足す」ができなくなる）
    /// - 全体で `MAX_PREVIEW_FEATURES` 枚を超えるなら、全体で最も長く使われていない 1 枚を捨てる
    /// 捨てたフレームの `/preview` は 409 `PREVIEW_FRAME_NOT_FOUND` になり、画面がそのフレームを登録し直す。
    pub fn save(&self, job_id: VideoJobId, time_ms: u32, features: Arc<PreviewFeatures>) {
        let key = (job_id, time_ms);
        let mut inner = self.lock();
        if !inner.entries.contains_key(&key) {
            let frames_of_job = inner.entries.keys().filter(|(id, _)| *id == job_id).count();
            if frames_of_job >= MAX_PROMPTS {
                inner.evict_oldest(|(id, _)| *id == job_id);
            } else if inner.entries.len() >= MAX_PREVIEW_FEATURES {
                inner.evict_oldest(|_| true);
            }
        }
        let tick = inner.next_tick();
        inner.entries.insert(key, (features, tick));
    }

    /// 取得したものは「最近使った」扱いになる。
    pub fn get(&self, job_id: &VideoJobId, time_ms: u32) -> Option<Arc<PreviewFeatures>> {
        let mut inner = self.lock();
        let tick = inner.next_tick();
        inner.entries.get_mut(&(*job_id, time_ms)).map(|(features, used)| {
            *used = tick;
            Arc::clone(features)
        })
    }

    /// そのジョブの全部の時刻の特徴量を消す。
    pub fn remove(&self, job_id: &VideoJobId) {
        self.lock().entries.retain(|(id, _), _| id != job_id);
    }

    /// `keep` が `false` を返した ID の特徴量を消す（ジョブが既に無いのに残ったものを掃除するため）。
    pub fn retain_ids(&self, keep: impl Fn(&VideoJobId) -> bool) {
        self.lock().entries.retain(|(id, _), _| keep(id));
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
        store.save(a, 0, features());
        store.save(b, 0, features());
        assert_eq!(store.get(&a, 0).unwrap().frame_width, 4);
        store.remove(&a);
        assert!(store.get(&a, 0).is_none());
        store.retain_ids(|id| *id != b);
        assert_eq!(store.len(), 0);
    }

    #[test]
    fn keeps_each_time_of_a_job_separately() {
        let store = PreviewFeatureStore::new();
        let a = VideoJobId::new_random();
        store.save(a, 1200, features());
        store.save(a, 4300, features());
        assert!(store.get(&a, 1200).is_some() && store.get(&a, 4300).is_some());
        assert!(store.get(&a, 5000).is_none());
        // 同じ時刻は置き換え（枚数は増えない）
        store.save(a, 4300, features());
        assert_eq!(store.len(), 2);
        // remove はそのジョブの全部の時刻を消す
        store.remove(&a);
        assert_eq!(store.len(), 0);
    }

    #[test]
    fn sixth_frame_of_a_job_evicts_its_least_recently_used_frame() {
        // 画面でフレームを消しても特徴量は残るので、拒否すると「消して別の時刻を足す」ができなくなる
        let store = PreviewFeatureStore::new();
        let (a, other) = (VideoJobId::new_random(), VideoJobId::new_random());
        store.save(other, 0, features());
        for t in 0..MAX_PROMPTS as u32 {
            store.save(a, t * 1000, features());
        }
        assert!(store.get(&a, 0).is_some()); // 0 を使う → 1000 が一番古い
        store.save(a, 9000, features());
        assert!(store.get(&a, 1000).is_none());
        assert!(store.get(&a, 0).is_some() && store.get(&a, 9000).is_some());
        assert!(store.get(&other, 0).is_some()); // 他のジョブは捨てない
        assert_eq!(store.len(), MAX_PROMPTS + 1);
    }

    #[test]
    fn evicts_least_recently_used_frame_beyond_the_cap() {
        let store = PreviewFeatureStore::new();
        let ids: Vec<VideoJobId> = (0..MAX_PREVIEW_FEATURES)
            .map(|_| VideoJobId::new_random())
            .collect();
        for id in &ids {
            store.save(*id, 0, features());
        }
        assert_eq!(store.len(), MAX_PREVIEW_FEATURES);
        // 一番古い ids[0] を使うと、次に古い ids[1] が捨てられる（枚単位）
        assert!(store.get(&ids[0], 0).is_some());
        store.save(ids[2], 500, features());
        assert_eq!(store.len(), MAX_PREVIEW_FEATURES);
        assert!(store.get(&ids[1], 0).is_none());
        assert!(store.get(&ids[0], 0).is_some());
        assert!(store.get(&ids[2], 0).is_some() && store.get(&ids[2], 500).is_some());
    }
}
