//! `POST /save` ハンドラ。高品質推論 + 保存用 WebP 生成を行い、その image_id を返す。
//! クライアントは続けて `/get-completed` でバイナリを取得する。

use std::sync::Arc;

use axum::{Json, extract::State};
use tokio::task::spawn_blocking;

use crate::{
    api::handler::save::{request::SaveRequest, response::SaveResponse},
    composition::wiring::App,
    editing::save::input::SaveInput,
    presentation::errors::{app_error::AppError, presentation_error::PresentationError},
    session::id::SessionId,
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

    // SAM2 推論・画像処理は CPU を長時間使うので、tokio のワーカーを塞がないよう
    // blocking スレッドで実行する。`??` の1つ目は JoinError（panic 等）、2つ目はユースケースのエラー。
    let output = spawn_blocking(move || app.save(input))
        .await
        .map_err(|err| PresentationError::BlockingTask(err.to_string()))??;

    let response = SaveResponse::new(*output.image_id().value());
    Ok(Json(response))
}
