//! `POST /internal/video-jobs/{id}/events` ハンドラ。ワーカーからの通知でジョブの状態を進める（利用ログには記録しない）。
//!
//! 順番: 署名の検証（401 `INVALID_SIGNATURE`）→ 本文を JSON として読む（400 `INVALID_REQUEST_BODY`）
//! → パスの ID と本文の `job_id` の照合（400 `JOB_ID_MISMATCH`）→ 反映して 204。
//! R2 の削除などの後処理はレスポンスを返す前に await する（Cloud Run のリクエストベースの CPU のため）。

use std::sync::Arc;

use axum::{
    body::Bytes,
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
};

use crate::{
    api::handler::video_cutout::{
        error::{VideoHandlerError, VideoRequestError},
        report_event::request::{SIGNATURE_HEADER, SIGNATURE_TIMESTAMP_HEADER},
    },
    composition::video_wiring::VideoCutoutApp,
    presentation::errors::app_error::AppError,
    video_cutout::{
        job::id::VideoJobId,
        report_event::input::{ReportVideoJobEventInput, WorkerEvent},
        signature::verify,
    },
};

pub async fn report_video_job_event(
    State(video): State<Arc<VideoCutoutApp>>,
    Path(id): Path<String>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<StatusCode, AppError> {
    let secret = video.callback_secret()?;
    let header = |name: &str| {
        headers
            .get(name)
            .and_then(|v| v.to_str().ok())
            .unwrap_or_default()
    };
    verify(
        secret,
        header(SIGNATURE_TIMESTAMP_HEADER),
        header(SIGNATURE_HEADER),
        &body,
        video.now_s(),
    )
    .map_err(VideoHandlerError::from)?;

    let event: WorkerEvent = serde_json::from_slice(&body)
        .map_err(|_| VideoHandlerError::from(VideoRequestError::InvalidBody))?;
    let job_id = VideoJobId::from_str(&id).map_err(VideoHandlerError::from)?;
    // 本文の job_id が UUID として読めない場合も「パスと違う」扱い
    if VideoJobId::from_str(&event.job_id).ok() != Some(job_id) {
        return Err(VideoHandlerError::from(VideoRequestError::JobIdMismatch).into());
    }

    video
        .report_video_job_event(ReportVideoJobEventInput { job_id, event })
        .await?;
    Ok(StatusCode::NO_CONTENT)
}
