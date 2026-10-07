use std::sync::Arc;

use axum::{Json, extract::State};
use tokio::task::spawn_blocking;

use crate::{
    application::usecase::segment_usecase::segment_input::SegmentInput,
    composition::wiring::App,
    domain::value_object::{
        coordinate::Coordinate,
        point::{Point, PointLabel},
        session_id::session_id::SessionId,
    },
    presentation::{
        errors::{app_error::AppError, presentation_error::PresentationError},
        handler::segment::{request::SegmentRequest, response::SegmentResponse},
    },
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

    let output = spawn_blocking(move || app.segment(input))
        .await
        .map_err(|err| PresentationError::BlockingTask(err.to_string()))??;

    let response = SegmentResponse::new(*output.image_id().value());
    Ok(Json(response))
}
