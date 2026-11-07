//! `POST /video-jobs/{id}/preview` ハンドラ。登録したフレームと点から、1 フレーム目のマスクを PNG で返す。
//!
//! ジョブの持ち主（`client-id`）だけ、`created` のときだけ使える。フレームが未登録なら 409 `PREVIEW_FRAME_NOT_FOUND`。
//! 回数は消費せず、利用ログにも記録しない。デコード（重い）はユースケースの中で `spawn_blocking` する。

use std::{net::SocketAddr, sync::Arc};

use axum::{
    Json,
    extract::{ConnectInfo, Path, State, rejection::JsonRejection},
    http::HeaderMap,
};

use crate::{
    api::handler::video_cutout::{
        error::{VideoHandlerError, VideoRequestError},
        preview_mask::{request::PreviewMaskRequest, response::PreviewMaskResponse},
        requester::Requester,
    },
    composition::video_wiring::VideoCutoutApp,
    presentation::errors::app_error::AppError,
    video_cutout::{job::id::VideoJobId, preview_mask::input::PreviewMaskInput},
};

pub async fn preview_video_mask(
    State(video): State<Arc<VideoCutoutApp>>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    Path(id): Path<String>,
    headers: HeaderMap,
    request: Result<Json<PreviewMaskRequest>, JsonRejection>,
) -> Result<PreviewMaskResponse, AppError> {
    video.ensure_enabled()?;
    let requester =
        Requester::from_request(&headers, peer, &video).map_err(VideoHandlerError::from)?;
    let job_id = VideoJobId::from_str(&id).map_err(VideoHandlerError::from)?;
    let Json(request) =
        request.map_err(|_| VideoHandlerError::from(VideoRequestError::InvalidBody))?;
    let points = request.points().map_err(VideoHandlerError::from)?;

    let output = video
        .preview_mask(PreviewMaskInput {
            client_id: requester.client_id,
            job_id,
            points,
        })
        .await?;
    Ok(PreviewMaskResponse {
        png: output.png,
        object_present: output.object_present,
    })
}
