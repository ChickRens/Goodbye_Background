//! `/get-completed` のリクエスト: `{ "image_id": "UUID文字列" }`（`/save` のレスポンスの image_id）。

#[derive(serde::Deserialize)]
pub struct GetCompletedRequest {
    pub image_id: String,
}
