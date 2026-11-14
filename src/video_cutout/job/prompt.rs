//! 点を打ったフレーム（複数フレームの点の指定。仕様 `docs/superpowers/specs/2026-11-09-video-multi-frame-prompts-design.md` §3.1）。

use crate::video_cutout::job::{clip_range::ClipRange, error::VideoJobError, point::Points};

/// 点を打てるフレームの数（開始位置を含む。オーナー判断）。
pub const MAX_PROMPTS: usize = 5;

/// 1 フレーム分の点。`time_ms` は元の動画の時刻（`ClipRange` と同じ基準）。
#[derive(Debug, PartialEq, Clone)]
pub struct Prompt {
    time_ms: u32,
    points: Points,
}

impl Prompt {
    pub fn new(time_ms: u32, points: Points) -> Self {
        Self { time_ms, points }
    }

    pub fn time_ms(&self) -> u32 {
        self.time_ms
    }

    pub fn points(&self) -> &Points {
        &self.points
    }
}

/// 1〜`MAX_PROMPTS` 個。1 つ目は範囲の開始、時刻の昇順で重複なし、`start <= time_ms < end`。
#[derive(Debug, PartialEq, Clone)]
pub struct Prompts(Vec<Prompt>);

impl Prompts {
    pub fn new(prompts: Vec<Prompt>, range: &ClipRange) -> Result<Self, VideoJobError> {
        let in_range = |t: u32| range.start_ms() <= t && t < range.end_ms();
        let ok = (1..=MAX_PROMPTS).contains(&prompts.len())
            && prompts[0].time_ms == range.start_ms()
            && prompts.iter().all(|p| in_range(p.time_ms))
            && prompts.windows(2).all(|w| w[0].time_ms < w[1].time_ms);
        if !ok {
            return Err(VideoJobError::InvalidPrompts);
        }
        Ok(Self(prompts))
    }

    pub fn as_slice(&self) -> &[Prompt] {
        &self.0
    }

    /// 開始位置の prompt（必ずある）。
    pub fn first(&self) -> &Prompt {
        &self.0[0]
    }
}

#[cfg(test)]
mod prompt_test {
    use super::*;
    use crate::video_cutout::job::{
        clip_range::ClipRange,
        error::VideoJobError,
        point::{NormalizedPoint, PointLabel, Points},
    };

    fn pts() -> Points {
        Points::new(vec![NormalizedPoint::new(0.5, 0.5, PointLabel::Include).unwrap()]).unwrap()
    }

    fn range() -> ClipRange {
        ClipRange::new(1200, 9200).unwrap()
    }

    fn at(times: &[u32]) -> Result<Prompts, VideoJobError> {
        Prompts::new(times.iter().map(|&t| Prompt::new(t, pts())).collect(), &range())
    }

    #[test]
    fn valid_prompts_pass() {
        assert_eq!(at(&[1200]).unwrap().as_slice().len(), 1);
        let five = at(&[1200, 2000, 3000, 4000, 9199]).unwrap();
        assert_eq!(five.as_slice().len(), MAX_PROMPTS);
        assert_eq!(five.first().time_ms(), 1200);
    }

    #[test]
    fn invalid_prompts_rejected() {
        for times in [
            &[][..],
            &[1200, 2000, 3000, 4000, 5000, 6000][..], // 6 個
            &[1300][..],                               // 1 つ目が開始位置でない
            &[1200, 1200][..],                         // 重複
            &[1200, 5000, 4000][..],                   // 降順
            &[1200, 9200][..],                         // 終わり以上
        ] {
            assert_eq!(at(times).unwrap_err(), VideoJobError::InvalidPrompts, "{times:?}");
        }
    }
}
