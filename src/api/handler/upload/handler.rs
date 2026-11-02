//! `POST /upload` ハンドラ。
//!
//! multipart の `file` フィールドを読み、`App::upload` を `spawn_blocking` で実行する。
//! 最初に見つかった `file` だけを処理して即 return するので、複数ファイルを送っても2枚目以降は無視される。
//! `file` 以外の名前のフィールドが先に来るとエラー（`UNKNOWN_TAG_NAME`）。
//!
//! ボディ上限は `router.rs` の `DefaultBodyLimit`（10MB）。

use std::sync::Arc;

use axum::{
    Json,
    extract::{Multipart, State},
};
use tokio::task::spawn_blocking;

use crate::{
    api::handler::upload::response::UploadResponse,
    composition::wiring::App,
    image_io::upload::input::UploadInput,
    presentation::errors::{app_error::AppError, presentation_error::PresentationError},
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

                // SAM2 推論・画像処理は CPU を長時間使うので、tokio のワーカーを塞がないよう
                // blocking スレッドで実行する。`??` の1つ目は JoinError（panic 等）、2つ目はユースケースのエラー。
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
