//! リクエストの結果。`Failed` にはクライアントに返したエラーコード（`Code::code()`）が入る。

#[derive(Clone)]
pub enum UsageStatus {
    Success,
    Failed(String),
}
