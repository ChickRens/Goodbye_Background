#[derive(serde::Deserialize)]
pub struct UndoRequest {
    pub session_id: String,
}
