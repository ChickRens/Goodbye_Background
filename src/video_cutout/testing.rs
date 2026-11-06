//! テスト用の偽物（外部サービスには繋がない）と、ユースケースのテストで共有するハーネス。

use std::collections::HashMap;
use std::net::IpAddr;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use async_trait::async_trait;
use uuid::Uuid;

use crate::video_cutout::{
    clock_impl::FakeVideoClock,
    create_job::{input::CreateVideoJobInput, usecase::CreateVideoJobUseCase},
    job::{
        background::Background,
        id::VideoJobId,
        locks::VideoJobLocks,
        phase::JobPhase,
        point::{NormalizedPoint, PointLabel, Points},
        repository::VideoJobRepository,
        repository_impl::VideoJobRepositoryInMemory,
    },
    quota::{
        daily_quota::{DAILY_LIMIT, DailyQuota, QuotaKeys},
        error::QuotaError,
        jst_day::jst_day,
    },
    start_job::{input::StartVideoJobInput, usecase::StartVideoJobUseCase},
    storage::{error::StorageError, video_object_storage::VideoObjectStorage},
    worker::{cutout_worker::CutoutWorker, error::WorkerError, request::WorkerJobRequest},
};

// ---- FakeStorage ----

/// URL は署名せず、キーと有効期間が読める形で返す。`head` の戻り値は設定できる。
pub struct FakeStorage {
    pub head_size: Mutex<Option<u64>>,
    pub deleted: Mutex<Vec<String>>,
}

impl FakeStorage {
    pub fn new(head_size: Option<u64>) -> Self {
        Self {
            head_size: Mutex::new(head_size),
            deleted: Mutex::new(Vec::new()),
        }
    }
}

#[async_trait]
impl VideoObjectStorage for FakeStorage {
    fn presign_put(&self, key: &str, content_type: &str, ttl: Duration) -> String {
        format!(
            "https://fake.r2/{key}?method=PUT&content-type={content_type}&X-Amz-Expires={}",
            ttl.as_secs()
        )
    }

    fn presign_get(&self, key: &str, ttl: Duration, download_filename: Option<&str>) -> String {
        let name = download_filename.map(|n| format!("&filename={n}")).unwrap_or_default();
        format!("https://fake.r2/{key}?method=GET&X-Amz-Expires={}{name}", ttl.as_secs())
    }

    async fn head(&self, _key: &str) -> Result<Option<u64>, StorageError> {
        Ok(*self.head_size.lock().unwrap())
    }

    async fn delete(&self, key: &str) -> Result<(), StorageError> {
        self.deleted.lock().unwrap().push(key.to_string());
        Ok(())
    }
}

// ---- FakeWorker ----

/// 投入された依頼を記録する。`fail` なら失敗、`delay` があればその分待ってから返す。
pub struct FakeWorker {
    pub dispatched: Mutex<Vec<WorkerJobRequest>>,
    pub fail: bool,
    pub delay: Option<Duration>,
}

impl FakeWorker {
    pub fn new() -> Self {
        Self {
            dispatched: Mutex::new(Vec::new()),
            fail: false,
            delay: None,
        }
    }

    pub fn failing() -> Self {
        Self { fail: true, ..Self::new() }
    }

    pub fn delayed(delay: Duration) -> Self {
        Self { delay: Some(delay), ..Self::new() }
    }
}

#[async_trait]
impl CutoutWorker for FakeWorker {
    async fn dispatch(&self, req: &WorkerJobRequest) -> Result<(), WorkerError> {
        if let Some(d) = self.delay {
            tokio::time::sleep(d).await;
        }
        self.dispatched.lock().unwrap().push(req.clone());
        if self.fail {
            return Err(WorkerError::Dispatch("fake failure".to_string()));
        }
        Ok(())
    }
}

// ---- InMemoryQuota ----

/// `DailyQuota` の HashMap 実装。キーは (day, key_kind, key)。
pub struct InMemoryQuota {
    counts: Mutex<HashMap<(i64, &'static str, String), i64>>,
}

impl InMemoryQuota {
    pub fn new() -> Self {
        Self { counts: Mutex::new(HashMap::new()) }
    }

    fn rows(keys: &QuotaKeys) -> [(&'static str, String); 2] {
        [("client", keys.client_id.to_string()), ("ip", keys.ip.to_string())]
    }

    fn remaining_locked(map: &HashMap<(i64, &'static str, String), i64>, keys: &QuotaKeys, day: i64) -> u8 {
        let used = Self::rows(keys)
            .into_iter()
            .map(|(kind, key)| *map.get(&(day, kind, key)).unwrap_or(&0))
            .max()
            .unwrap_or(0);
        (DAILY_LIMIT - used).clamp(0, DAILY_LIMIT) as u8
    }
}

#[async_trait]
impl DailyQuota for InMemoryQuota {
    async fn remaining(&self, keys: &QuotaKeys, day: i64) -> Result<u8, QuotaError> {
        Ok(Self::remaining_locked(&self.counts.lock().unwrap(), keys, day))
    }

    async fn try_consume(&self, keys: &QuotaKeys, day: i64) -> Result<u8, QuotaError> {
        let mut map = self.counts.lock().unwrap();
        if Self::remaining_locked(&map, keys, day) == 0 {
            return Err(QuotaError::Exceeded("daily limit".to_string()));
        }
        for (kind, key) in Self::rows(keys) {
            *map.entry((day, kind, key)).or_insert(0) += 1;
        }
        Ok(Self::remaining_locked(&map, keys, day))
    }

    async fn refund(&self, keys: &QuotaKeys, day: i64) -> Result<(), QuotaError> {
        let mut map = self.counts.lock().unwrap();
        for (kind, key) in Self::rows(keys) {
            let c = map.entry((day, kind, key)).or_insert(0);
            *c = (*c - 1).max(0);
        }
        Ok(())
    }
}

// ---- ユースケースのハーネス ----

/// ハーネスの時計の初期値（UNIX ミリ秒）。
pub const NOW_MS: u64 = 1_700_000_000_000;
pub const PUBLIC_BASE_URL: &str = "https://example.test";

pub fn client_id() -> Uuid {
    Uuid::from_u128(0x1234_5678_9abc_def0_1234_5678_9abc_def0)
}

pub fn client_ip() -> IpAddr {
    "203.0.113.7".parse().unwrap()
}

pub fn quota_keys() -> QuotaKeys {
    QuotaKeys { client_id: client_id(), ip: client_ip() }
}

pub fn create_input() -> CreateVideoJobInput {
    CreateVideoJobInput {
        client_id: client_id(),
        ip: client_ip(),
        size_bytes: 10_000_000,
        content_type: "video/mp4".to_string(),
        start_ms: 1_000,
        end_ms: 9_000,
    }
}

pub fn start_input(job_id: VideoJobId) -> StartVideoJobInput {
    StartVideoJobInput {
        client_id: client_id(),
        ip: client_ip(),
        job_id,
        points: Points::new(vec![NormalizedPoint::new(0.5, 0.4, PointLabel::Include).unwrap()])
            .unwrap(),
        background: Background::Green,
    }
}

pub struct Harness {
    pub create: CreateVideoJobUseCase,
    pub start: StartVideoJobUseCase,
    pub storage: Arc<FakeStorage>,
    pub worker: Arc<FakeWorker>,
    pub quota: Arc<InMemoryQuota>,
    pub repository: Arc<VideoJobRepositoryInMemory>,
    pub clock: Arc<FakeVideoClock>,
}

impl Harness {
    pub fn build(storage: FakeStorage, worker: FakeWorker, quota: InMemoryQuota) -> Self {
        let storage = Arc::new(storage);
        let worker = Arc::new(worker);
        let quota = Arc::new(quota);
        let repository = Arc::new(VideoJobRepositoryInMemory::new());
        let locks = Arc::new(VideoJobLocks::new());
        let clock = Arc::new(FakeVideoClock::new(NOW_MS));
        let create = CreateVideoJobUseCase::new(
            storage.clone(),
            repository.clone(),
            quota.clone(),
            clock.clone(),
        );
        let start = StartVideoJobUseCase::new(
            PUBLIC_BASE_URL.to_string(),
            storage.clone(),
            worker.clone(),
            repository.clone(),
            locks.clone(),
            quota.clone(),
            clock.clone(),
        );
        Self { create, start, storage, worker, quota, repository, clock }
    }

    /// `create_input()` でジョブを作り、その ID を返す。
    pub async fn created_job(&self) -> VideoJobId {
        self.create.execute(create_input()).await.unwrap().job_id
    }

    pub async fn remaining(&self) -> u8 {
        self.quota.remaining(&quota_keys(), jst_day(NOW_MS)).await.unwrap()
    }

    pub fn phase(&self, id: VideoJobId) -> JobPhase {
        self.repository.get(&id).unwrap().phase
    }
}

/// アップロード済み（HEAD が 1MB を返す）・ワーカー正常・回数未使用。
pub fn harness() -> Harness {
    harness_with(FakeWorker::new())
}

pub fn harness_with(worker: FakeWorker) -> Harness {
    Harness::build(FakeStorage::new(Some(1_000_000)), worker, InMemoryQuota::new())
}

pub fn harness_with_head(head_size: Option<u64>) -> Harness {
    Harness::build(FakeStorage::new(head_size), FakeWorker::new(), InMemoryQuota::new())
}

/// 今日すでに `used` 回使った状態。
pub fn harness_with_used(used: usize) -> Harness {
    let quota = InMemoryQuota::new();
    {
        let mut map = quota.counts.lock().unwrap();
        for (kind, key) in InMemoryQuota::rows(&quota_keys()) {
            map.insert((jst_day(NOW_MS), kind, key), used as i64);
        }
    }
    Harness::build(FakeStorage::new(Some(1_000_000)), FakeWorker::new(), quota)
}
