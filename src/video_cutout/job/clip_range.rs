//! 切り抜く区間（ミリ秒）。最大 10 秒。

use crate::video_cutout::job::error::VideoJobError;

pub const MAX_CLIP_MS: u32 = 10_000;

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub struct ClipRange {
    start_ms: u32,
    end_ms: u32,
}

impl ClipRange {
    /// `start_ms < end_ms` かつ長さが `MAX_CLIP_MS` 以下のときだけ作れる。
    pub fn new(start_ms: u32, end_ms: u32) -> Result<Self, VideoJobError> {
        if end_ms <= start_ms || end_ms - start_ms > MAX_CLIP_MS {
            return Err(VideoJobError::InvalidRange);
        }
        Ok(Self { start_ms, end_ms })
    }

    pub fn start_ms(&self) -> u32 {
        self.start_ms
    }

    pub fn end_ms(&self) -> u32 {
        self.end_ms
    }

    #[cfg(test)]
    pub fn duration_ms(&self) -> u32 {
        self.end_ms - self.start_ms
    }
}

#[cfg(test)]
mod clip_range_test {
    use super::*;

    #[test]
    fn clip_range_accepts_exactly_10s() {
        assert!(ClipRange::new(0, 10_000).is_ok());
    }

    #[test]
    fn clip_range_rejects_over_10s_and_reversed() {
        assert_eq!(ClipRange::new(0, 10_001), Err(VideoJobError::InvalidRange));
        assert_eq!(ClipRange::new(500, 500), Err(VideoJobError::InvalidRange));
        assert_eq!(ClipRange::new(900, 100), Err(VideoJobError::InvalidRange));
    }

    #[test]
    fn accessors() {
        let r = ClipRange::new(1_000, 4_000).unwrap();
        assert_eq!((r.start_ms(), r.end_ms(), r.duration_ms()), (1_000, 4_000, 3_000));
    }
}
