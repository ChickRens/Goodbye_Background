use std::sync::Arc;

use axum::{Json, extract::State};
use tokio::task::spawn_blocking;

use crate::{
    application::usecase::save_usecase::input::SaveInput,
    composition::wiring::App,
    domain::value_object::session_id::session_id::SessionId,
    presentation::{
        errors::{app_error::AppError, presentation_error::PresentationError},
        handler::save::{request::SaveRequest, response::SaveResponse},
    },
};

#[axum::debug_handler]
pub async fn save(
    State(app): State<Arc<App>>,
    Json(request): Json<SaveRequest>,
) -> Result<Json<SaveResponse>, AppError> {
    let session_id = request.session_id;
    let session_id =
        SessionId::from_str(&session_id).map_err(|err| PresentationError::from(err))?;

    let input = SaveInput::new(session_id);

    let output = spawn_blocking(move || app.save(input))
        .await
        .map_err(|err| PresentationError::BlockingTask(err.to_string()))??;

    let response = SaveResponse::new(*output.image_id().value());
    Ok(Json(response))
}
