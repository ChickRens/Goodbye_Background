//! `POST /video-jobs/{id}/preview-frame` のリクエスト: multipart のフィールド `file`（PNG / JPEG、10MB 以下、長辺 2000px 以下）。
//!
//! 画像の中身の検証はユースケース（`set_preview_frame::usecase`）で行う。ここでは `file` のバイト列を取り出すだけ。
//! 本文の上限（`router.rs` の `DefaultBodyLimit`）を超えたときなど、multipart を読めなければ `INVALID_PREVIEW_IMAGE`。

use axum::extract::Multipart;

/// クエリ `?time_ms=4300`（元の動画のミリ秒）。
#[derive(serde::Deserialize)]
pub struct PreviewFrameQuery {
    pub time_ms: u32,
}

use crate::video_cutout::preview::error::PreviewInputError;

/// 最初の `file` フィールドのバイト列を返す。それ以外の名前のフィールドは読み飛ばす。
pub async fn read_file_field(multipart: &mut Multipart) -> Result<Vec<u8>, PreviewInputError> {
    while let Some(field) = multipart
        .next_field()
        .await
        .map_err(|_| PreviewInputError::InvalidImage)?
    {
        if field.name() == Some("file") {
            let bytes = field
                .bytes()
                .await
                .map_err(|_| PreviewInputError::InvalidImage)?;
            return Ok(bytes.to_vec());
        }
    }
    Err(PreviewInputError::InvalidImage)
}
