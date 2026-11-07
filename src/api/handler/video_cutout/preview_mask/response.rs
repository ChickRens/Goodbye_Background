//! `POST /video-jobs/{id}/preview` のレスポンス（200）: `image/png`（グレースケール 8bit の alpha マスク。255 = 前景）。
//! 大きさは登録したフレームと同じ。
//! ヘッダー `X-Object-Present: true | false`: 1 フレーム目に対象が写っているか（`object_score_logits > 0`）。
//! `false` なら、このまま開始するとワーカーは `subject_not_found` で失敗する。

use axum::{
    body::Body,
    http::{HeaderName, header::CONTENT_TYPE},
    response::{IntoResponse, Response},
};

pub struct PreviewMaskResponse {
    pub png: Vec<u8>,
    pub object_present: bool,
}

impl IntoResponse for PreviewMaskResponse {
    fn into_response(self) -> Response {
        let object_present = if self.object_present { "true" } else { "false" };
        (
            [
                (CONTENT_TYPE, "image/png"),
                (HeaderName::from_static("x-object-present"), object_present),
            ],
            Body::from(self.png),
        )
            .into_response()
    }
}
