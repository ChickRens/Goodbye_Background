//! `POST /video-jobs/{id}/start` のリクエスト。
//!
//! ```json
//! { "points": [{ "x": 0.5, "y": 0.4, "label": "include" }], "background": "green" }
//! ```
//! `x`/`y` は動画フレームに対する 0.0〜1.0。`label` は `include` / `exclude`、`background` は `green` / `blue` / `magenta`。
//! 値の検証はドメイン（`NormalizedPoint` / `Points` / `Background`）で行う。

use crate::video_cutout::job::{
    background::Background,
    error::VideoJobError,
    point::{NormalizedPoint, PointLabel, Points},
};

#[derive(serde::Deserialize)]
pub struct StartVideoJobRequest {
    pub points: Vec<PointRequest>,
    pub background: String,
}

#[derive(serde::Deserialize)]
pub struct PointRequest {
    pub x: f32,
    pub y: f32,
    pub label: String,
}

impl StartVideoJobRequest {
    pub fn points(&self) -> Result<Points, VideoJobError> {
        let points = self
            .points
            .iter()
            .map(|p| {
                let label = match p.label.as_str() {
                    "include" => PointLabel::Include,
                    "exclude" => PointLabel::Exclude,
                    _ => return Err(VideoJobError::InvalidPoints),
                };
                NormalizedPoint::new(p.x, p.y, label)
            })
            .collect::<Result<Vec<_>, _>>()?;
        Points::new(points)
    }

    pub fn background(&self) -> Result<Background, VideoJobError> {
        Background::from_str(&self.background)
    }
}
