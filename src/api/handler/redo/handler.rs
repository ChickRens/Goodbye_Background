use std::sync::Arc;

use axum::{Json, extract::State};
use tokio::task::spawn_blocking;

use crate::{
    api::handler::redo::{request::RedoRequest, response::RedoResponse},
    composition::wiring::App,
    editing::redo::input::RedoInput,
    presentation::errors::{app_error::AppError, presentation_error::PresentationError},
    session::id::SessionId,
};

#[axum::debug_handler]
pub async fn redo(
    State(app): State<Arc<App>>,
    Json(request): Json<RedoRequest>,
) -> Result<Json<RedoResponse>, AppError> {
    let session_id = request.session_id;
    let session_id =
        SessionId::from_str(&session_id).map_err(|err| PresentationError::from(err))?;

    let input = RedoInput::new(session_id);

    let output = spawn_blocking(move || app.redo(input))
        .await
        .map_err(|err| PresentationError::BlockingTask(err.to_string()))??;

    let response = RedoResponse::new(*output.image_id().value());
    Ok(Json(response))
}
