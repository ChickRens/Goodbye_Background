//! ジョブの状態を HTTP レスポンスの文字列にする（`phase` は小文字、`failure_code` は `FailureCode::as_str`）。

use crate::video_cutout::job::phase::JobPhase;

pub fn phase_name(phase: &JobPhase) -> &'static str {
    match phase {
        JobPhase::Created => "created",
        JobPhase::Queued => "queued",
        JobPhase::Downloading => "downloading",
        JobPhase::Tracking(_) => "tracking",
        JobPhase::Finishing => "finishing",
        JobPhase::Done => "done",
        JobPhase::Failed(_) => "failed",
    }
}

/// `Failed` のときだけ失敗コード。
pub fn failure_code(phase: &JobPhase) -> Option<&'static str> {
    match phase {
        JobPhase::Failed(code) => Some(code.as_str()),
        _ => None,
    }
}

#[cfg(test)]
mod phase_test {
    use super::*;
    use crate::video_cutout::job::phase::FailureCode;

    #[test]
    fn names_are_lowercase_strings() {
        assert_eq!(phase_name(&JobPhase::Tracking(0.5)), "tracking");
        assert_eq!(
            phase_name(&JobPhase::Failed(FailureCode::Timeout)),
            "failed"
        );
        assert_eq!(
            failure_code(&JobPhase::Failed(FailureCode::Timeout)),
            Some("timeout")
        );
        assert_eq!(failure_code(&JobPhase::Done), None);
    }
}
