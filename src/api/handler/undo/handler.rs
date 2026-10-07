use std::sync::Arc;

use axum::{Json, extract::State};
use tokio::task::spawn_blocking;

use crate::{
    application::usecase::undo_usecase::undo_input::UndoInput,
    composition::wiring::App,
    domain::value_object::session_id::session_id::SessionId,
    presentation::{
        errors::{app_error::AppError, presentation_error::PresentationError},
        handler::undo::{request::UndoRequest, response::UndoResponse},
    },
};

#[axum::debug_handler]
pub async fn undo(
    State(app): State<Arc<App>>,
    Json(request): Json<UndoRequest>,
) -> Result<Json<UndoResponse>, AppError> {
    let session_id = request.session_id;
    let session_id =
        SessionId::from_str(&session_id).map_err(|err| PresentationError::from(err))?;

    let input = UndoInput::new(session_id);

    let output = spawn_blocking(move || app.undo(input))
        .await
        .map_err(|err| PresentationError::BlockingTask(err.to_string()))??;

    let response = UndoResponse::new(*output.image_id().value());
    Ok(Json(response))
}
