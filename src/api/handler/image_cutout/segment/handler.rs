//! `POST /segment` ハンドラ。クリック1点を追加して切り抜き直す。
//!
//! 座標 `x`, `y` は **プレビュー画像（長辺600px）上のピクセル座標**。元画像座標への変換は
//! `CommonEditingSession::points_with` / `apply_edit` で行われる（`point / point_scale`）。
//! 範囲外の座標のバリデーションは無い。

use std::sync::Arc;

use axum::{Json, extract::State};
use tokio::task::spawn_blocking;

use crate::{
    api::handler::image_cutout::segment::{request::SegmentRequest, response::SegmentResponse},
    composition::wiring::App,
    image_cutout::editing::segment::input::SegmentInput,
    image_cutout::point::{
        coordinate::Coordinate,
        point::{Point, PointLabel},
    },
    presentation::errors::{app_error::AppError, presentation_error::PresentationError},
    image_cutout::session::id::SessionId,
};

#[axum::debug_handler]
pub async fn segment(
    State(app): State<Arc<App>>,
    Json(request): Json<SegmentRequest>,
) -> Result<Json<SegmentResponse>, AppError> {
    let session_id = request.session_id;
    let x = request.x;
    let y = request.y;
    let is_foreground = request.is_foreground;
    let session_id =
        SessionId::from_str(&session_id).map_err(|err| PresentationError::from(err))?;

    let label = match is_foreground {
        true => PointLabel::FOREGROUND,
        false => PointLabel::BACKGROUND,
    };

    let point = Point::new(Coordinate::new(x, y), label);
    let input = SegmentInput::new(session_id, point);

    // SAM2 推論・画像処理は CPU を長時間使うので、tokio のワーカーを塞がないよう
    // blocking スレッドで実行する。`??` の1つ目は JoinError（panic 等）、2つ目はユースケースのエラー。
    let output = spawn_blocking(move || app.segment(input))
        .await
        .map_err(|err| PresentationError::BlockingTask(err.to_string()))??;

    let response = SegmentResponse::new(*output.image_id().value());
    Ok(Json(response))
}
