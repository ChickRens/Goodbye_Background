use std::sync::Arc;

use axum::{Json, body::Body, extract::State, response::Response};

use crate::{
    api::handler::get_preview::request::GetImageRequest,
    composition::wiring::App,
    image::image::id::ImageId,
    image_io::get_preview::input::GetPreviewInput,
    presentation::errors::{app_error::AppError, presentation_error::PresentationError},
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
