//! ジョブの進行状態と失敗コード。

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum FailureCode {
    DispatchFailed,
    Timeout,
    InternalError,
    SubjectNotFound,
    UnsupportedVideo,
    RangeOutOfBounds,
    TooLarge,
}

impl FailureCode {
    pub fn as_str(&self) -> &'static str {
        match self {
            FailureCode::DispatchFailed => "dispatch_failed",
            FailureCode::Timeout => "timeout",
            FailureCode::InternalError => "internal_error",
            FailureCode::SubjectNotFound => "subject_not_found",
            FailureCode::UnsupportedVideo => "unsupported_video",
            FailureCode::RangeOutOfBounds => "range_out_of_bounds",
            FailureCode::TooLarge => "too_large",
        }
    }

    /// 失敗時にクォータ（利用回数）を返却するか。クライアント起因の 2 つだけ返さない。
    pub fn refunds_quota(&self) -> bool {
        !matches!(
            self,
            FailureCode::RangeOutOfBounds | FailureCode::TooLarge
        )
    }

    /// ワーカーが報告できる 5 つのコードだけを受け付ける（dispatch_failed / timeout はサーバー側で付ける）。
    pub fn from_worker(s: &str) -> Option<Self> {
        match s {
            "internal_error" => Some(FailureCode::InternalError),
            "subject_not_found" => Some(FailureCode::SubjectNotFound),
            "unsupported_video" => Some(FailureCode::UnsupportedVideo),
            "range_out_of_bounds" => Some(FailureCode::RangeOutOfBounds),
            "too_large" => Some(FailureCode::TooLarge),
            _ => None,
        }
    }
}

#[derive(Debug, PartialEq, Clone, Copy)]
pub enum JobPhase {
    Created,
    Queued,
    Downloading,
    Tracking(f32),
    Finishing,
    Done,
    Failed(FailureCode),
}

impl JobPhase {
    pub fn is_terminal(&self) -> bool {
        matches!(self, JobPhase::Done | JobPhase::Failed(_))
    }

    /// 通常フェーズの順序（Failed は別扱い）。
    fn rank(&self) -> u8 {
        match self {
            JobPhase::Created => 0,
            JobPhase::Queued => 1,
            JobPhase::Downloading => 2,
            JobPhase::Tracking(_) => 3,
            JobPhase::Finishing => 4,
            JobPhase::Done => 5,
            JobPhase::Failed(_) => 6,
        }
    }

    /// 前に進むときだけ `Some(next)`。イベントの間引き・欠落を考慮し、途中のフェーズを飛ばしてもよい。
    /// `Tracking` 同士は進捗が増えるときだけ。最終状態からは常に `None`。
    pub fn advance(&self, next: JobPhase) -> Option<JobPhase> {
        if self.is_terminal() {
            return None;
        }
        // 進捗が非有限・範囲外の Tracking は受け付けない（NaN/inf が入ると進捗が止まるため）
        if let JobPhase::Tracking(p) = next {
            if !p.is_finite() || !(0.0..=1.0).contains(&p) {
                return None;
            }
        }
        match (self, &next) {
            (_, JobPhase::Failed(_)) => Some(next),
            (JobPhase::Tracking(cur), JobPhase::Tracking(new)) => (new > cur).then_some(next),
            _ => (next.rank() > self.rank()).then_some(next),
        }
    }
}

#[cfg(test)]
mod phase_test {
    use super::*;

    #[test]
    fn phase_only_moves_forward() {
        assert_eq!(JobPhase::Tracking(0.5).advance(JobPhase::Tracking(0.4)), None);
        assert_eq!(JobPhase::Tracking(0.5).advance(JobPhase::Tracking(0.5)), None);
        assert_eq!(
            JobPhase::Tracking(0.5).advance(JobPhase::Tracking(0.6)),
            Some(JobPhase::Tracking(0.6))
        );
        assert_eq!(JobPhase::Finishing.advance(JobPhase::Downloading), None);
        assert_eq!(JobPhase::Finishing.advance(JobPhase::Tracking(0.9)), None);
        assert_eq!(JobPhase::Done.advance(JobPhase::Failed(FailureCode::Timeout)), None);
        assert_eq!(
            JobPhase::Failed(FailureCode::Timeout).advance(JobPhase::Done),
            None
        );
        assert_eq!(
            JobPhase::Queued.advance(JobPhase::Failed(FailureCode::InternalError)),
            Some(JobPhase::Failed(FailureCode::InternalError))
        );
    }

    #[test]
    fn tracking_progress_must_be_finite_and_in_range() {
        for bad in [f32::NAN, f32::INFINITY, -0.1, 1.1] {
            assert!(JobPhase::Queued.advance(JobPhase::Tracking(bad)).is_none());
        }
        assert_eq!(
            JobPhase::Queued.advance(JobPhase::Tracking(1.0)),
            Some(JobPhase::Tracking(1.0))
        );
    }

    #[test]
    fn phase_may_skip_forward() {
        assert_eq!(
            JobPhase::Queued.advance(JobPhase::Tracking(0.2)),
            Some(JobPhase::Tracking(0.2))
        );
        assert_eq!(JobPhase::Created.advance(JobPhase::Queued), Some(JobPhase::Queued));
        assert_eq!(JobPhase::Tracking(0.9).advance(JobPhase::Finishing), Some(JobPhase::Finishing));
        assert_eq!(JobPhase::Queued.advance(JobPhase::Queued), None);
    }

    #[test]
    fn terminal_flags() {
        assert!(JobPhase::Done.is_terminal());
        assert!(JobPhase::Failed(FailureCode::Timeout).is_terminal());
        assert!(!JobPhase::Tracking(0.1).is_terminal());
    }

    #[test]
    fn refund_table_matches_spec() {
        use FailureCode::*;
        for c in [DispatchFailed, Timeout, InternalError, SubjectNotFound, UnsupportedVideo] {
            assert!(c.refunds_quota());
        }
        for c in [RangeOutOfBounds, TooLarge] {
            assert!(!c.refunds_quota());
        }
    }

    #[test]
    fn failure_code_strings_and_worker_parse() {
        assert_eq!(FailureCode::DispatchFailed.as_str(), "dispatch_failed");
        assert_eq!(FailureCode::RangeOutOfBounds.as_str(), "range_out_of_bounds");
        assert_eq!(FailureCode::from_worker("too_large"), Some(FailureCode::TooLarge));
        assert_eq!(FailureCode::from_worker("timeout"), None);
        assert_eq!(FailureCode::from_worker("dispatch_failed"), None);
        assert_eq!(FailureCode::from_worker("x"), None);
    }
}
