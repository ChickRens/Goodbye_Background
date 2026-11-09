//! テスト用の偽物（外部サービスには繋がない）と、ユースケースのテストで共有するハーネス。

use std::collections::HashMap;
use std::net::IpAddr;
use std::ops::Deref;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use async_trait::async_trait;
use half::f16;
use image::{ImageFormat, RgbImage};
use ndarray::{Array2, Array4};
use uuid::Uuid;

use crate::video_cutout::{
    clock::VideoClock,
    clock_impl::FakeVideoClock,
    create_job::{input::CreateVideoJobInput, usecase::CreateVideoJobUseCase},
    delete_expired_jobs::usecase::DeleteExpiredVideoJobsUseCase,
    get_job::{input::GetVideoJobInput, output::GetVideoJobOutput, usecase::GetVideoJobUseCase},
    job::{
        background::Background,
        id::VideoJobId,
        locks::VideoJobLocks,
        phase::JobPhase,
        point::{NormalizedPoint, PointLabel, Points},
        prompt::Prompt,
        repository::VideoJobRepository,
        repository_impl::VideoJobRepositoryInMemory,
    },
    preview::{
        error::PreviewError,
        feature_store::PreviewFeatureStore,
        segmenter::{DEFAULT_PREVIEW_INPUT_SIZE, PreviewDecoded, PreviewFeatures, PreviewSegmenter},
    },
    preview_mask::{input::PreviewMaskInput, usecase::PreviewMaskUseCase},
    quota::{
        daily_quota::{DAILY_LIMIT, DailyQuota, QuotaKeys},
        error::QuotaError,
        jst_day::jst_day,
    },
    report_event::{
        input::{ReportVideoJobEventInput, WorkerEvent},
        usecase::ReportVideoJobEventUseCase,
    },
    set_preview_frame::{input::SetPreviewFrameInput, usecase::SetPreviewFrameUseCase},
    start_job::{input::StartVideoJobInput, usecase::StartVideoJobUseCase},
    storage::{error::StorageError, video_object_storage::VideoObjectStorage},
    worker::{cutout_worker::CutoutWorker, error::WorkerError, request::WorkerJobRequest},
};

// ---- FakeStorage ----

/// URL は署名せず、キーと有効期間が読める形で返す。`head` の戻り値は設定できる。
/// `set_fail_delete(true)` にすると `delete` が失敗する（削除済みには記録しない）。
pub struct FakeStorage {
    pub head_size: Mutex<Option<u64>>,
    pub deleted: Mutex<Vec<String>>,
    fail_delete: AtomicBool,
}

impl FakeStorage {
    pub fn new(head_size: Option<u64>) -> Self {
        Self {
            head_size: Mutex::new(head_size),
            deleted: Mutex::new(Vec::new()),
            fail_delete: AtomicBool::new(false),
        }
    }

    /// 削除に成功したキー（呼ばれた順）。
    pub fn deleted(&self) -> Vec<String> {
        self.deleted.lock().unwrap().clone()
    }

    pub fn set_fail_delete(&self, fail: bool) {
        self.fail_delete.store(fail, Ordering::SeqCst);
    }
}

#[async_trait]
impl VideoObjectStorage for FakeStorage {
    fn presign_put(
        &self,
        key: &str,
        content_type: &str,
        content_length: Option<u64>,
        ttl: Duration,
    ) -> String {
        let len = content_length.map(|n| format!("&content-length={n}")).unwrap_or_default();
        format!(
            "https://fake.r2/{key}?method=PUT&content-type={content_type}{len}&X-Amz-Expires={}",
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
        if self.fail_delete.load(Ordering::SeqCst) {
            return Err(StorageError::Request("fake delete failure".to_string()));
        }
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

// ---- FakePreviewSegmenter ----

/// エンコードの回数とデコードに渡された点を記録する。
/// デコードは左半分（x < 256）を前景（ロジット +10）、右半分を背景（-10）にした 512x512 を返す。
/// 対象のスコアは既定で +1（写っている）。`set_object_score` で変えられる。
pub struct FakePreviewSegmenter {
    encoded: AtomicUsize,
    decoded: Mutex<Vec<Points>>,
    object_score: Mutex<f32>,
}

impl FakePreviewSegmenter {
    pub fn new() -> Self {
        Self {
            encoded: AtomicUsize::new(0),
            decoded: Mutex::new(Vec::new()),
            object_score: Mutex::new(1.0),
        }
    }

    pub fn encoded(&self) -> usize {
        self.encoded.load(Ordering::SeqCst)
    }

    pub fn decoded_points(&self) -> Vec<Points> {
        self.decoded.lock().unwrap().clone()
    }

    pub fn set_object_score(&self, score: f32) {
        *self.object_score.lock().unwrap() = score;
    }
}

impl PreviewSegmenter for FakePreviewSegmenter {
    fn encode(&self, frame: &RgbImage) -> Result<PreviewFeatures, PreviewError> {
        self.encoded.fetch_add(1, Ordering::SeqCst);
        Ok(PreviewFeatures {
            feats0: Array4::from_elem((1, 1, 1, 1), f16::ZERO),
            feats1: Array4::from_elem((1, 1, 1, 1), f16::ZERO),
            feats2_no_mem: Array4::from_elem((1, 1, 1, 1), f16::ZERO),
            frame_width: frame.width(),
            frame_height: frame.height(),
        })
    }

    fn decode(&self, _features: &PreviewFeatures, points: &Points) -> Result<PreviewDecoded, PreviewError> {
        self.decoded.lock().unwrap().push(points.clone());
        let s = DEFAULT_PREVIEW_INPUT_SIZE;
        Ok(PreviewDecoded {
            logits: Array2::from_shape_fn((s, s), |(_, x)| if x < s / 2 { 10.0 } else { -10.0 }),
            object_score: *self.object_score.lock().unwrap(),
        })
    }
}

/// `width` x `height` の PNG（中身は適当な模様）。
pub fn png_bytes(width: u32, height: u32) -> Vec<u8> {
    encoded_frame(width, height, ImageFormat::Png)
}

pub fn jpeg_bytes(width: u32, height: u32) -> Vec<u8> {
    encoded_frame(width, height, ImageFormat::Jpeg)
}

fn encoded_frame(width: u32, height: u32, format: ImageFormat) -> Vec<u8> {
    let img = RgbImage::from_fn(width, height, |x, y| image::Rgb([x as u8, y as u8, 128]));
    let mut bytes = Vec::new();
    img.write_to(&mut std::io::Cursor::new(&mut bytes), format).unwrap();
    bytes
}

pub fn frame_input(job_id: VideoJobId, image: Vec<u8>) -> SetPreviewFrameInput {
    SetPreviewFrameInput { client_id: client_id(), job_id, time_ms: START_MS, image }
}

pub fn mask_input(job_id: VideoJobId) -> PreviewMaskInput {
    PreviewMaskInput {
        client_id: client_id(),
        job_id,
        time_ms: START_MS,
        points: Points::new(vec![
            NormalizedPoint::new(0.25, 0.5, PointLabel::Include).unwrap(),
            NormalizedPoint::new(0.75, 0.5, PointLabel::Exclude).unwrap(),
        ])
        .unwrap(),
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
/// `create_input` の範囲の開始（1 枚目のプレビューのフレームの時刻）。
pub const START_MS: u32 = 1_000;
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
        start_ms: START_MS,
        end_ms: 9_000,
    }
}

pub fn start_input(job_id: VideoJobId) -> StartVideoJobInput {
    StartVideoJobInput {
        client_id: client_id(),
        ip: client_ip(),
        job_id,
        prompts: vec![Prompt::new(
            START_MS,
            Points::new(vec![NormalizedPoint::new(0.5, 0.4, PointLabel::Include).unwrap()]).unwrap(),
        )],
        background: Background::Green,
    }
}

pub fn get_input(job_id: VideoJobId) -> GetVideoJobInput {
    GetVideoJobInput { client_id: client_id(), ip: client_ip(), job_id }
}

/// 通知の JSON の `job_id`（ユースケースは見ないので固定値）。
const EVENT_JOB_ID: &str = "6f1c2b9e-0d4a-4c1e-9a57-3f2e8b1d7c60";

pub fn tracking(progress: f32) -> serde_json::Value {
    serde_json::json!({"job_id": EVENT_JOB_ID, "phase": "tracking", "progress": progress})
}

pub fn failed(code: &str) -> serde_json::Value {
    serde_json::json!({"job_id": EVENT_JOB_ID, "phase": "failed", "code": code})
}

pub fn done() -> serde_json::Value {
    serde_json::json!({"job_id": EVENT_JOB_ID, "phase": "done", "frames": 30, "lost_frames": 0, "duration_ms": 1000})
}

pub struct Harness {
    pub create: CreateVideoJobUseCase,
    pub start: StartVideoJobUseCase,
    pub get: GetVideoJobUseCase,
    pub report_uc: ReportVideoJobEventUseCase,
    pub cleanup: DeleteExpiredVideoJobsUseCase,
    pub set_frame: SetPreviewFrameUseCase,
    pub preview_mask: PreviewMaskUseCase,
    pub preview: Arc<FakePreviewSegmenter>,
    pub features: Arc<PreviewFeatureStore>,
    pub storage: Arc<FakeStorage>,
    pub worker: Arc<FakeWorker>,
    pub quota: Arc<InMemoryQuota>,
    pub repository: Arc<VideoJobRepositoryInMemory>,
    pub locks: Arc<VideoJobLocks>,
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
        let preview = Arc::new(FakePreviewSegmenter::new());
        let features = Arc::new(PreviewFeatureStore::new());
        let set_frame = SetPreviewFrameUseCase::new(
            preview.clone(),
            repository.clone(),
            locks.clone(),
            features.clone(),
        );
        let preview_mask =
            PreviewMaskUseCase::new(preview.clone(), repository.clone(), features.clone());
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
            features.clone(),
        );
        let get = GetVideoJobUseCase::new(
            storage.clone(),
            repository.clone(),
            locks.clone(),
            quota.clone(),
            clock.clone(),
        );
        let report_uc = ReportVideoJobEventUseCase::new(
            storage.clone(),
            repository.clone(),
            locks.clone(),
            quota.clone(),
            clock.clone(),
        );
        let cleanup = DeleteExpiredVideoJobsUseCase::new(
            storage.clone(),
            repository.clone(),
            locks.clone(),
            quota.clone(),
            features.clone(),
        );
        Self {
            create,
            start,
            get,
            report_uc,
            cleanup,
            set_frame,
            preview_mask,
            preview,
            features,
            storage,
            worker,
            quota,
            repository,
            locks,
            clock,
        }
    }

    pub fn clock_now(&self) -> u64 {
        self.clock.now_ms()
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

/// 開始済み（`Queued`、回数を 1 回消費）のジョブ 1 つと、そのハーネス。
pub struct StartedJob {
    pub h: Harness,
    pub id: VideoJobId,
}

impl Deref for StartedJob {
    type Target = Harness;

    fn deref(&self) -> &Harness {
        &self.h
    }
}

impl StartedJob {
    pub fn id_str(&self) -> String {
        self.id.value().to_string()
    }

    pub fn input_key(&self) -> String {
        format!("video-jobs/{}/input", self.id_str())
    }

    pub fn output_key(&self) -> String {
        format!("video-jobs/{}/output.mp4", self.id_str())
    }

    /// 通知の JSON をデシリアライズしてこのジョブに反映する。
    pub async fn report(&self, event: serde_json::Value) {
        let event: WorkerEvent = serde_json::from_value(event).unwrap();
        self.h
            .report_uc
            .execute(ReportVideoJobEventInput { job_id: self.id, event })
            .await
            .unwrap();
    }

    pub async fn get(&self) -> GetVideoJobOutput {
        self.h.get.execute(get_input(self.id)).await.unwrap()
    }

    /// ハーネスの時計の現在時刻で期限切れの削除を実行する。
    pub async fn cleanup(&self) -> usize {
        self.h.cleanup.execute(self.h.clock_now()).await
    }
}

pub async fn started_job() -> StartedJob {
    let h = harness();
    let id = h.created_job().await;
    h.start.execute(start_input(id)).await.unwrap();
    StartedJob { h, id }
}
