//! `POST /video-jobs` ハンドラ。ジョブを作り、入力動画のアップロード先 URL を返す（回数は消費しない）。

use std::{net::SocketAddr, sync::Arc};

use axum::{
    Json,
    extract::{ConnectInfo, State, rejection::JsonRejection},
    http::{HeaderMap, StatusCode},
};

use crate::{
    api::handler::video_cutout::{
        create_job::{request::CreateVideoJobRequest, response::CreateVideoJobResponse},
        error::{VideoHandlerError, VideoRequestError},
        requester::Requester,
    },
    composition::video_wiring::VideoCutoutApp,
    presentation::{client_ip::client_ip_debug_line, errors::app_error::AppError},
    video_cutout::create_job::input::CreateVideoJobInput,
};

pub async fn create_video_job(
    State(video): State<Arc<VideoCutoutApp>>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    request: Result<Json<CreateVideoJobRequest>, JsonRejection>,
) -> Result<(StatusCode, Json<CreateVideoJobResponse>), AppError> {
    video.ensure_enabled()?;
    // 一時的な確認用（LOG_CLIENT_IP_DEBUG=1）。TRUSTED_PROXY_HOPS を決めたら環境変数を外す
    if let Some(line) = client_ip_debug_line(
        video.log_client_ip_debug(),
        &headers,
        peer,
        video.trusted_proxy_hops(),
    ) {
        println!("{line}");
    }
    let requester =
        Requester::from_request(&headers, peer, &video).map_err(VideoHandlerError::from)?;
    let Json(request) =
        request.map_err(|_| VideoHandlerError::from(VideoRequestError::InvalidBody))?;

    let output = video
        .create_video_job(CreateVideoJobInput {
            client_id: requester.client_id,
            ip: requester.ip,
            size_bytes: request.size_bytes,
            content_type: request.content_type,
            start_ms: request.start_ms,
            end_ms: request.end_ms,
        })
        .await?;

    Ok((
        StatusCode::CREATED,
        Json(CreateVideoJobResponse {
            job_id: *output.job_id.value(),
            upload_url: output.upload_url,
            remaining_today: output.remaining_today,
        }),
    ))
}
