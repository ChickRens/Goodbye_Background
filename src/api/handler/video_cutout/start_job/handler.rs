//! `POST /video-jobs/{id}/start` ハンドラ。アップロード済みの動画で切り抜きを始める（回数を消費する）。
//!
//! ワーカーへの投入に失敗しても 202 で `phase: "failed"` を返す（エラーにはしない）。

use std::{net::SocketAddr, sync::Arc};

use axum::{
    Json,
    extract::{ConnectInfo, Path, State, rejection::JsonRejection},
    http::{HeaderMap, StatusCode},
};

use crate::{
    api::handler::video_cutout::{
        error::{VideoHandlerError, VideoRequestError},
        phase::{failure_code, phase_name},
        requester::Requester,
        start_job::{request::StartVideoJobRequest, response::StartVideoJobResponse},
    },
    composition::video_wiring::VideoCutoutApp,
    presentation::errors::app_error::AppError,
    video_cutout::{job::id::VideoJobId, start_job::input::StartVideoJobInput},
};

pub async fn start_video_job(
    State(video): State<Arc<VideoCutoutApp>>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    Path(id): Path<String>,
    headers: HeaderMap,
    request: Result<Json<StartVideoJobRequest>, JsonRejection>,
) -> Result<(StatusCode, Json<StartVideoJobResponse>), AppError> {
    video.ensure_enabled()?;
    let requester =
        Requester::from_request(&headers, peer, &video).map_err(VideoHandlerError::from)?;
    let job_id = VideoJobId::from_str(&id).map_err(VideoHandlerError::from)?;
    let Json(request) =
        request.map_err(|_| VideoHandlerError::from(VideoRequestError::InvalidBody))?;
    let points = request.points().map_err(VideoHandlerError::from)?;
    let background = request.background().map_err(VideoHandlerError::from)?;

    let output = video
        .start_video_job(StartVideoJobInput {
            client_id: requester.client_id,
            ip: requester.ip,
            job_id,
            points,
            background,
        })
        .await?;

    Ok((
        StatusCode::ACCEPTED,
        Json(StartVideoJobResponse {
            phase: phase_name(&output.phase),
            failure_code: failure_code(&output.phase),
            remaining_today: output.remaining_today,
        }),
    ))
}
