//! `POST /video-jobs/{id}/preview-frame` ハンドラ。プレビュー用のフレーム（1 枚目の静止画）を登録する。
//!
//! ジョブの持ち主（`client-id`）だけ、`created` のときだけ使える。回数は消費せず、利用ログにも記録しない。
//! エンコード（重い）はユースケースの中で `spawn_blocking` する。成功は 204。
//! 本文を読む前に同時実行の許可を取る（空きが無ければ 503 `PREVIEW_BUSY`）。

use std::{net::SocketAddr, sync::Arc};

use axum::{
    extract::{ConnectInfo, Multipart, Path, State, multipart::MultipartRejection},
    http::{HeaderMap, StatusCode},
};

use crate::{
    api::handler::video_cutout::{
        error::VideoHandlerError, requester::Requester, set_preview_frame::request::read_file_field,
    },
    composition::video_wiring::VideoCutoutApp,
    presentation::errors::app_error::AppError,
    video_cutout::{
        job::id::VideoJobId, preview::error::PreviewInputError,
        set_preview_frame::input::SetPreviewFrameInput,
    },
};

pub async fn set_preview_frame(
    State(video): State<Arc<VideoCutoutApp>>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    Path(id): Path<String>,
    headers: HeaderMap,
    multipart: Result<Multipart, MultipartRejection>,
) -> Result<StatusCode, AppError> {
    video.ensure_enabled()?;
    let requester =
        Requester::from_request(&headers, peer, &video).map_err(VideoHandlerError::from)?;
    let job_id = VideoJobId::from_str(&id).map_err(VideoHandlerError::from)?;
    // 登録が終わるまで持つ（本文の読み込み・デコード・エンコードの全部を数える）
    let _permit = video.try_acquire_preview_encode()?;
    let mut multipart =
        multipart.map_err(|_| VideoHandlerError::from(PreviewInputError::InvalidImage))?;
    let image = read_file_field(&mut multipart)
        .await
        .map_err(VideoHandlerError::from)?;

    video
        .set_preview_frame(SetPreviewFrameInput {
            client_id: requester.client_id,
            job_id,
            image,
        })
        .await?;
    Ok(StatusCode::NO_CONTENT)
}
