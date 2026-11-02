//! どの API が呼ばれたか。URL パス（先頭の `/` を除いたもの）から変換する。
//! ミドルウェアは `router.rs` で API ルートにだけ掛かっている（fallback の静的配信は後から追加しているので対象外）
//! ため、現状 `Unknown` になるのは新しいルートを追加して変換表を更新し忘れた場合くらい。
//! ルートを追加したら、ここと `recorder.rs` の文字列変換の両方を更新すること。

pub enum UsageOperation {
    Upload,
    Segment,
    Undo,
    Redo,
    Save,
    GetCompleted,
    GetPreview,
    Unknown(String),
}

impl UsageOperation {
    pub fn new(operation: &str) -> Self {
        match operation {
            "upload" => Self::Upload,
            "segment" => Self::Segment,
            "save" => Self::Save,
            "undo" => Self::Undo,
            "redo" => Self::Redo,
            "get-completed" => Self::GetCompleted,
            "get-preview" => Self::GetPreview,
            unknown => Self::Unknown(unknown.to_string()),
        }
    }
}
