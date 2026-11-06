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
    /// `POST /video-jobs`
    CreateVideoJob,
    /// `POST /video-jobs/{id}/start`
    StartVideoJob,
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
            "video-jobs" => Self::CreateVideoJob,
            // `GET /video-jobs/{id}` と `/internal/...` は track_usage の外なのでここには来ない
            path if path.starts_with("video-jobs/") && path.ends_with("/start") => {
                Self::StartVideoJob
            }
            unknown => Self::Unknown(unknown.to_string()),
        }
    }
}

#[cfg(test)]
mod operation_test {
    use super::UsageOperation;

    #[test]
    fn usage_operation_maps_video_paths() {
        assert!(matches!(UsageOperation::new("video-jobs"), UsageOperation::CreateVideoJob));
        assert!(matches!(
            UsageOperation::new("video-jobs/6f1c2b9e-0d4a-4c1e-9a57-3f2e8b1d7c60/start"),
            UsageOperation::StartVideoJob
        ));
        assert!(matches!(UsageOperation::new("video-jobs/x"), UsageOperation::Unknown(_)));
    }
}
