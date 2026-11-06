//! 動画切り抜きの依存性の組み立てと、そのファサード `VideoCutoutApp`。
//!
//! 画像用の `App`（wiring.rs）とは分けてある（設計判断 R4）。`App::new` は SAM2 の ONNX モデルを読むので、
//! 動画だけをテストしたいときにモデル無しで組み立てられるようにするため。動画のハンドラは
//! `State<Arc<VideoCutoutApp>>` を受け取り、このメソッドだけを呼ぶ（ユースケースを直接触らない）。
//!
//! 動画のポート（R2・Modal・回数の DB）はネットワーク待ちだけの非同期 trait なので、
//! 画像と違って `spawn_blocking` は使わない（CLAUDE.md の例外）。
//!
//! 設定（`VideoCutoutConfig`）が無ければ何も持たず、全メソッドが `VIDEO_CUTOUT_DISABLED`（503）を返す。

use std::sync::Arc;

use crate::{
    application::error::ApplicationError,
    video_cutout::{
        clock::VideoClock,
        clock_impl::RealVideoClock,
        config::VideoCutoutConfig,
        create_job::{
            input::CreateVideoJobInput, output::CreateVideoJobOutput,
            usecase::CreateVideoJobUseCase,
        },
        delete_expired_jobs::{
            throttle::VideoCleanupThrottle, usecase::DeleteExpiredVideoJobsUseCase,
        },
        error::VideoCutoutError,
        get_job::{
            input::GetVideoJobInput, output::GetVideoJobOutput, usecase::GetVideoJobUseCase,
        },
        job::{locks::VideoJobLocks, repository_impl::VideoJobRepositoryInMemory},
        quota::daily_quota::DailyQuota,
        report_event::{
            input::ReportVideoJobEventInput, output::ReportVideoJobEventOutput,
            usecase::ReportVideoJobEventUseCase,
        },
        start_job::{
            input::StartVideoJobInput, output::StartVideoJobOutput, usecase::StartVideoJobUseCase,
        },
        storage::{
            r2_video_object_storage::R2VideoObjectStorage, video_object_storage::VideoObjectStorage,
        },
        worker::{cutout_worker::CutoutWorker, modal_cutout_worker::ModalCutoutWorker},
    },
};

/// 動画切り抜きのファサード。axum の `State<Arc<VideoCutoutApp>>` として動画のハンドラに渡される。
pub struct VideoCutoutApp {
    /// 設定が無い（動画が無効）なら `None`。
    inner: Option<VideoCutout>,
}

/// 有効なときの中身。ストア・ロック・時計は全ユースケースで共有する。
struct VideoCutout {
    create: CreateVideoJobUseCase,
    start: StartVideoJobUseCase,
    get: GetVideoJobUseCase,
    report: ReportVideoJobEventUseCase,
    cleanup: DeleteExpiredVideoJobsUseCase,
    throttle: VideoCleanupThrottle,
    clock: Arc<dyn VideoClock>,
    callback_secret: Vec<u8>,
    trusted_proxy_hops: usize,
}

impl VideoCutoutApp {
    /// `quota` は利用ログと同じ DB の `SqliteDailyQuota` / `PostgresDailyQuota`（main.rs で作る）。
    pub fn new(quota: Arc<dyn DailyQuota>, config: Option<VideoCutoutConfig>) -> Self {
        let Some(config) = config else {
            return Self { inner: None };
        };
        let storage: Arc<dyn VideoObjectStorage> = Arc::new(R2VideoObjectStorage::new(
            config.r2_account_id,
            config.r2_bucket,
            config.r2_access_key_id,
            config.r2_secret_access_key,
        ));
        let worker: Arc<dyn CutoutWorker> = Arc::new(ModalCutoutWorker::new(
            config.modal_endpoint_url,
            config.modal_key,
            config.modal_secret,
        ));
        Self {
            inner: Some(VideoCutout::build(
                storage,
                worker,
                quota,
                Arc::new(RealVideoClock),
                config.public_base_url,
                config.worker_callback_secret.into_bytes(),
                config.trusted_proxy_hops,
            )),
        }
    }

    /// 偽物の R2・ワーカー・回数・時計で組み立てる（ルーターのテスト用。SAM2 のモデルは要らない）。
    #[cfg(test)]
    pub fn for_test(
        storage: Arc<crate::video_cutout::testing::FakeStorage>,
        worker: Arc<crate::video_cutout::testing::FakeWorker>,
        quota: Arc<crate::video_cutout::testing::InMemoryQuota>,
        clock: Arc<crate::video_cutout::clock_impl::FakeVideoClock>,
        callback_secret: &str,
    ) -> Self {
        Self {
            inner: Some(VideoCutout::build(
                storage,
                worker,
                quota,
                clock,
                crate::video_cutout::testing::PUBLIC_BASE_URL.to_string(),
                callback_secret.as_bytes().to_vec(),
                0,
            )),
        }
    }

    pub fn is_enabled(&self) -> bool {
        self.inner.is_some()
    }

    /// 無効なら `VIDEO_CUTOUT_DISABLED`。ハンドラが本文を読む前に呼ぶ（無効時は本文に関わらず 503 にするため）。
    pub fn ensure_enabled(&self) -> Result<(), ApplicationError> {
        self.enabled().map(|_| ())
    }

    pub async fn create_video_job(
        &self,
        input: CreateVideoJobInput,
    ) -> Result<CreateVideoJobOutput, ApplicationError> {
        let video = self.enabled()?;
        video.maybe_cleanup().await;
        Ok(video.create.execute(input).await?)
    }

    pub async fn start_video_job(
        &self,
        input: StartVideoJobInput,
    ) -> Result<StartVideoJobOutput, ApplicationError> {
        let video = self.enabled()?;
        video.maybe_cleanup().await;
        Ok(video.start.execute(input).await?)
    }

    pub async fn get_video_job(
        &self,
        input: GetVideoJobInput,
    ) -> Result<GetVideoJobOutput, ApplicationError> {
        let video = self.enabled()?;
        video.maybe_cleanup().await;
        Ok(video.get.execute(input).await?)
    }

    pub async fn report_video_job_event(
        &self,
        input: ReportVideoJobEventInput,
    ) -> Result<ReportVideoJobEventOutput, ApplicationError> {
        let video = self.enabled()?;
        video.maybe_cleanup().await;
        Ok(video.report.execute(input).await?)
    }

    /// 期限切れのジョブを今すぐ片付ける（main.rs の GC タスク用。スロットルを通さない）。無効なら何もしない。
    pub async fn cleanup_video_jobs(&self) {
        if let Some(video) = &self.inner {
            video.cleanup_now().await;
        }
    }

    /// ワーカーからの通知の HMAC 鍵。
    pub fn callback_secret(&self) -> Result<&[u8], ApplicationError> {
        Ok(&self.enabled()?.callback_secret)
    }

    /// `X-Forwarded-For` を右から何番目まで信頼するか。無効なら 0（IP は使われない）。
    pub fn trusted_proxy_hops(&self) -> usize {
        self.inner.as_ref().map_or(0, |v| v.trusted_proxy_hops)
    }

    /// 署名の時刻のずれの判定に使う現在時刻（UNIX 秒）。テストでは偽の時計になる。
    pub fn now_s(&self) -> u64 {
        match &self.inner {
            Some(video) => video.clock.now_ms() / 1000,
            None => RealVideoClock.now_ms() / 1000,
        }
    }

    fn enabled(&self) -> Result<&VideoCutout, ApplicationError> {
        self.inner
            .as_ref()
            .ok_or_else(|| VideoCutoutError::Disabled.into())
    }
}

impl VideoCutout {
    fn build(
        storage: Arc<dyn VideoObjectStorage>,
        worker: Arc<dyn CutoutWorker>,
        quota: Arc<dyn DailyQuota>,
        clock: Arc<dyn VideoClock>,
        public_base_url: String,
        callback_secret: Vec<u8>,
        trusted_proxy_hops: usize,
    ) -> Self {
        // ジョブは状態ごとの期限で DeleteExpiredVideoJobsUseCase が消す（TtlStore の GC には登録しない）
        let repository = Arc::new(VideoJobRepositoryInMemory::new());
        let locks = Arc::new(VideoJobLocks::new());
        Self {
            create: CreateVideoJobUseCase::new(
                storage.clone(),
                repository.clone(),
                quota.clone(),
                clock.clone(),
            ),
            start: StartVideoJobUseCase::new(
                public_base_url,
                storage.clone(),
                worker,
                repository.clone(),
                locks.clone(),
                quota.clone(),
                clock.clone(),
            ),
            get: GetVideoJobUseCase::new(
                storage.clone(),
                repository.clone(),
                locks.clone(),
                quota.clone(),
                clock.clone(),
            ),
            report: ReportVideoJobEventUseCase::new(
                storage.clone(),
                repository.clone(),
                locks.clone(),
                quota.clone(),
                clock.clone(),
            ),
            cleanup: DeleteExpiredVideoJobsUseCase::new(storage, repository, locks, quota),
            throttle: VideoCleanupThrottle::new(),
            clock,
            callback_secret,
            trusted_proxy_hops,
        }
    }

    /// 最後の実行から 60 秒以上経っていれば、期限切れのジョブを片付ける。
    /// Cloud Run はリクエストベースの CPU で裏の GC がほとんど進まないため、動画 API の処理中に await する（仕様 §4.9）。
    async fn maybe_cleanup(&self) {
        if self.throttle.should_run(self.clock.now_ms()) {
            self.cleanup_now().await;
        }
    }

    async fn cleanup_now(&self) {
        let processed = self.cleanup.execute(self.clock.now_ms()).await;
        if processed > 0 {
            println!("期限切れの動画ジョブを {processed} 件処理しました");
        }
    }
}
