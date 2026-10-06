use std::sync::Arc;

use axum::{Json, body::Body, extract::State, response::Response};

use crate::{
    application::usecase::get_preview_usecase::get_preview_input::GetPreviewInput,
    composition::wiring::App,
    domain::value_object::image_id::image_id::ImageId,
    presentation::{
        errors::{app_error::AppError, presentation_error::PresentationError},
        handler::get_preview::request::GetImageRequest,
    },
};

#[axum::debug_handler]
pub async fn get_preview(
    State(app): State<Arc<App>>,
    Json(request): Json<GetImageRequest>,
) -> Result<Response, AppError> {
    println!("Getting image start");
    let image_id = request.image_id;
    let image_id = ImageId::from_str(&image_id).map_err(|err| PresentationError::from(err))?;

    let input = GetPreviewInput::new(image_id);

    let output = app.get_preview(input).await?;

    let (image, _) = output.image_data();

    Ok(Response::builder()
        .header("Content-Type", "image/webp")
        .body(Body::from(image))
        .unwrap())
}
