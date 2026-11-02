//! `/get-preview` のリクエスト: `{ "image_id": "UUID文字列" }`。
//! 型名が `GetImageRequest` なのは旧名の名残。

#[derive(serde::Deserialize)]
pub struct GetImageRequest {
    pub image_id: String,
}
