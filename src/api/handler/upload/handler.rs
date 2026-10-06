use std::sync::Arc;

use axum::{
    Json,
    extract::{Multipart, State},
};
use tokio::task::spawn_blocking;

use crate::{
    application::usecase::upload_usecase::upload_input::UploadInput,
    composition::wiring::App,
    presentation::{
        errors::{app_error::AppError, presentation_error::PresentationError},
        handler::upload::response::UploadResponse,
    },
};

#[axum::debug_handler]
pub async fn upload(
    State(app): State<Arc<App>>,
    mut multipart: Multipart,
) -> Result<Json<UploadResponse>, AppError> {
    while let Some(field) = multipart
        .next_field()
        .await
        .map_err(|err| PresentationError::Multipart(err.to_string()))?
    {
        match field.name() {
            Some("file") => {
                let bytes = field
                    .bytes()
                    .await
                    .map_err(|err| PresentationError::Multipart(err.to_string()))?;

                println!("Uploading");

                let input = UploadInput::new(bytes.to_vec());

                let output = spawn_blocking(move || app.upload(input))
                    .await
                    .map_err(|err| PresentationError::BlockingTask(err.to_string()))??;

                let (session_id, image_id) = output.session_id_and_image_id();
                let response = UploadResponse::new(*session_id.value(), *image_id.value());

                return Ok(Json(response));
            }

            Some(unknown) => {
                return Err(PresentationError::UnknownName(unknown.to_string()).into());
            }

            None => {
                Err(PresentationError::NoName)?;
            }
        }
    }
    Err(PresentationError::Multipart("Multipart Failed".to_string()))?
}
