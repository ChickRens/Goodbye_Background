//! `POST /video-jobs/{id}/preview` のレスポンス（200）: `image/png`（グレースケール 8bit の alpha マスク。255 = 前景）。
//! 大きさは登録したフレームと同じ。

use axum::{
    body::Body,
    http::header::CONTENT_TYPE,
    response::{IntoResponse, Response},
};

pub struct PreviewMaskResponse {
    pub png: Vec<u8>,
}

impl IntoResponse for PreviewMaskResponse {
    fn into_response(self) -> Response {
        ([(CONTENT_TYPE, "image/png")], Body::from(self.png)).into_response()
    }
}
