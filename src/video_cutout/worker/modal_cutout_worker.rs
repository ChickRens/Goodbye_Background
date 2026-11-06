//! `CutoutWorker` の Modal 実装。Python の存在が現れるのはこのファイルだけ（仕様 §4.2）。
//!
//! Modal の Web エンドポイントに依頼の JSON を POST する。プロキシ認証は `Modal-Key` / `Modal-Secret` ヘッダー。

use std::time::Duration;

use async_trait::async_trait;

use super::{cutout_worker::CutoutWorker, error::WorkerError, request::WorkerJobRequest};

/// 投入リクエストのタイムアウト。
const DISPATCH_TIMEOUT: Duration = Duration::from_secs(30);

pub struct ModalCutoutWorker {
    client: reqwest::Client,
    endpoint_url: String,
    modal_key: String,
    modal_secret: String,
}

impl ModalCutoutWorker {
    pub fn new(endpoint_url: String, modal_key: String, modal_secret: String) -> Self {
        let client = reqwest::Client::builder()
            .timeout(DISPATCH_TIMEOUT)
            .build()
            .expect("reqwest クライアントの生成に失敗");
        Self {
            client,
            endpoint_url,
            modal_key,
            modal_secret,
        }
    }
}

#[async_trait]
impl CutoutWorker for ModalCutoutWorker {
    /// 依頼を POST する。reqwest のエラーは `without_url()` で URL を落としてから文字列にする。
    ///
    /// タイムアウト（`DISPATCH_TIMEOUT`）は「届かなかった」とは限らない。Modal 側で受け付け済みなら
    /// ワーカーはそのまま動き続けることがある。その場合もジョブは `Failed(dispatch_failed)`（回数は返却）になり、
    /// 後から届く通知は最終状態のジョブなので無視される。ワーカーが書いた出力は期限切れの削除で消える。
    async fn dispatch(&self, req: &WorkerJobRequest) -> Result<(), WorkerError> {
        let res = self
            .client
            .post(&self.endpoint_url)
            .header("Modal-Key", &self.modal_key)
            .header("Modal-Secret", &self.modal_secret)
            .json(req)
            .send()
            .await
            .map_err(|e| WorkerError::Dispatch(e.without_url().to_string()))?;
        if !res.status().is_success() {
            return Err(WorkerError::Dispatch(format!("status {}", res.status())));
        }
        Ok(())
    }
}
