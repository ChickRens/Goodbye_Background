//! `/save` のリクエスト: `{ "session_id": "UUID文字列" }`。

#[derive(serde::Deserialize)]
pub struct SaveRequest {
    pub session_id: String,
}
