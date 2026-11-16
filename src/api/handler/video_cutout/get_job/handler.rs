//! `GET /video-jobs/{id}` ハンドラ。ジョブの状態を返す（フロントが 1 秒ごとにポーリングする。利用ログには記録しない）。

use std::{net::SocketAddr, sync::Arc};

use axum::{
    Json,
    extract::{ConnectInfo, Path, State},
    http::HeaderMap,
};

use crate::{
    api::handler::video_cutout::{
        error::VideoHandlerError, get_job::response::GetVideoJobResponse, phase::phase_name,
        requester::Requester,
    },
    composition::video_wiring::VideoCutoutApp,
    presentation::errors::app_error::AppError,
    video_cutout::{
        get_job::input::GetVideoJobInput,
        job::{id::VideoJobId, phase::JobPhase},
    },
};

pub async fn get_video_job(
    State(video): State<Arc<VideoCutoutApp>>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    Path(id): Path<String>,
    headers: HeaderMap,
) -> Result<Json<GetVideoJobResponse>, AppError> {
    video.ensure_enabled()?;
    let requester =
        Requester::from_request(&headers, peer, &video).map_err(VideoHandlerError::from)?;
    let job_id = VideoJobId::from_str(&id).map_err(VideoHandlerError::from)?;

    let output = video
        .get_video_job(GetVideoJobInput {
            client_id: requester.client_id,
            ip: requester.ip,
            job_id,
        })
        .await?;

    let progress = match output.phase {
        JobPhase::Tracking(p) => Some(p),
        _ => None,
    };
    Ok(Json(GetVideoJobResponse {
        phase: phase_name(&output.phase),
        progress,
        failure_code: output.failure.map(|code| code.as_str()),
        quota_refunded: output.quota_refunded,
        download_url: output.download_url,
        remaining_today: output.remaining_today,
    }))
}
