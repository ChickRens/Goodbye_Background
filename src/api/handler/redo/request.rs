//! `/redo` のリクエスト: `{ "session_id": "UUID文字列" }`。

#[derive(serde::Deserialize)]
pub struct RedoRequest {
    pub session_id: String,
}
