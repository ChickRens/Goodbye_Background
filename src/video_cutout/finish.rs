//! ジョブを最終状態（`Done` / `Failed`）にする共通処理。通知・状態の取得・期限切れの削除の 3 つから使う。
//!
//! 呼び出し側がジョブ単位のロックを持ったまま呼ぶこと。

use crate::video_cutout::{
    job::{phase::JobPhase, repository::VideoJobRepository, video_job::VideoJob},
    quota::{
        daily_quota::{DailyQuota, QuotaKeys},
        jst_day::jst_day,
    },
    storage::video_object_storage::VideoObjectStorage,
};

/// `phase`（最終状態）にして保存し、入力ファイルを削除する（仕様 §5.3 の 1 段目）。
///
/// - `Failed` で返却の対象（仕様 §5.1）なら、消費済みかつ未返却のときだけ回数を返して `quota_refunded = true` にする。
///   返却は持ち主のキー（作成時の client_id と IP）で、消費した日（`quota_day`。無ければ今日、日本時間）の分を戻す。
/// - 返却・削除の失敗はログだけ（ジョブの状態は進める）。
/// - 削除はこの呼び出しの中で await する（Cloud Run はリクエスト外で CPU が割り当てられないため）。
pub async fn finish_job(
    mut job: VideoJob,
    phase: JobPhase,
    now_ms: u64,
    repository: &dyn VideoJobRepository,
    storage: &dyn VideoObjectStorage,
    quota: &dyn DailyQuota,
) -> VideoJob {
    debug_assert!(phase.is_terminal());
    job.phase = phase;
    job.last_event_at_ms = now_ms;

    if let JobPhase::Failed(code) = phase {
        if code.refunds_quota() && job.quota_consumed && !job.quota_refunded {
            let keys = QuotaKeys {
                client_id: job.owner_client_id,
                ip: job.owner_ip,
            };
            // 消費した日の分を戻す（記録が無い古いジョブだけ今日の分）
            let day = job.quota_day.unwrap_or_else(|| jst_day(now_ms));
            match quota.refund(&keys, day).await {
                Ok(()) => job.quota_refunded = true,
                Err(e) => eprintln!("回数の返却に失敗 job={}: {:?}", job.id.value(), e),
            }
        }
    }

    repository.save(job.clone());
    if let Err(e) = storage.delete(&job.input_key()).await {
        eprintln!("入力ファイルの削除に失敗 job={}: {:?}", job.id.value(), e);
    }
    job
}
