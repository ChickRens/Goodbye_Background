//! 追跡対象を指定するクリック点（動画フレームに対する 0.0〜1.0 の正規化座標）。

use crate::video_cutout::job::error::VideoJobError;

pub const MAX_POINTS: usize = 8;

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum PointLabel {
    Include,
    Exclude,
}

#[derive(Debug, PartialEq, Clone, Copy)]
pub struct NormalizedPoint {
    x: f32,
    y: f32,
    label: PointLabel,
}

impl NormalizedPoint {
    /// x, y は 0.0〜1.0（有限値）でなければならない。
    pub fn new(x: f32, y: f32, label: PointLabel) -> Result<Self, VideoJobError> {
        let ok = |v: f32| v.is_finite() && (0.0..=1.0).contains(&v);
        if !ok(x) || !ok(y) {
            return Err(VideoJobError::InvalidPoints);
        }
        Ok(Self { x, y, label })
    }

    pub fn x(&self) -> f32 {
        self.x
    }

    pub fn y(&self) -> f32 {
        self.y
    }

    pub fn label(&self) -> PointLabel {
        self.label
    }
}

#[derive(Debug, PartialEq, Clone)]
pub struct Points(Vec<NormalizedPoint>);

impl Points {
    /// 1〜`MAX_POINTS` 個。
    pub fn new(points: Vec<NormalizedPoint>) -> Result<Self, VideoJobError> {
        if points.is_empty() || points.len() > MAX_POINTS {
            return Err(VideoJobError::InvalidPoints);
        }
        Ok(Self(points))
    }

    pub fn as_slice(&self) -> &[NormalizedPoint] {
        &self.0
    }
}

#[cfg(test)]
mod point_test {
    use super::*;

    fn p() -> NormalizedPoint {
        NormalizedPoint::new(0.5, 0.5, PointLabel::Include).unwrap()
    }

    #[test]
    fn points_bounds() {
        assert!(NormalizedPoint::new(1.01, 0.5, PointLabel::Include).is_err());
        assert!(Points::new(vec![]).is_err());
        assert!(Points::new(vec![p(); 9]).is_err());
        assert!(Points::new(vec![p(); 1]).is_ok());
        assert!(Points::new(vec![p(); 8]).is_ok());
    }

    #[test]
    fn point_rejects_negative_and_nan() {
        assert!(NormalizedPoint::new(-0.01, 0.5, PointLabel::Exclude).is_err());
        assert!(NormalizedPoint::new(0.5, f32::NAN, PointLabel::Exclude).is_err());
        assert!(NormalizedPoint::new(0.0, 1.0, PointLabel::Exclude).is_ok());
    }
}
