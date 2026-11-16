//! `/undo` のリクエスト: `{ "session_id": "UUID文字列" }`。

#[derive(serde::Deserialize)]
pub struct UndoRequest {
    pub session_id: String,
}
