//! `POST /video-jobs/{id}/start` のリクエスト。
//!
//! ```json
//! {
//!   "prompts": [
//!     { "time_ms": 1200, "points": [{ "x": 0.5, "y": 0.4, "label": "include" }] },
//!     { "time_ms": 4300, "points": [{ "x": 0.6, "y": 0.4, "label": "include" }] }
//!   ],
//!   "background": "green"
//! }
//! ```
//! `prompts` は点を打ったフレーム（1〜5 個、1 つ目は範囲の開始、昇順で重複なし、範囲の中）。規則はユースケースが
//! `Prompts::new` で検査する（範囲はジョブにあるため）。
//! `x`/`y` は動画フレームに対する 0.0〜1.0。`label` は `include` / `exclude`、`background` は `green` / `blue` / `magenta`。
//! 値の検証はドメイン（`NormalizedPoint` / `Points` / `Background`）で行う。

use crate::video_cutout::job::{
    background::Background,
    error::VideoJobError,
    point::{NormalizedPoint, PointLabel, Points},
    prompt::Prompt,
};

#[derive(serde::Deserialize)]
pub struct StartVideoJobRequest {
    pub prompts: Vec<PromptRequest>,
    pub background: String,
}

#[derive(serde::Deserialize)]
pub struct PromptRequest {
    pub time_ms: u32,
    pub points: Vec<PointRequest>,
}

#[derive(serde::Deserialize)]
pub struct PointRequest {
    pub x: f32,
    pub y: f32,
    pub label: String,
}

impl StartVideoJobRequest {
    /// 各フレームの点を検査する（フレームの並びの規則はユースケースで）。
    pub fn prompts(&self) -> Result<Vec<Prompt>, VideoJobError> {
        self.prompts
            .iter()
            .map(|p| Ok(Prompt::new(p.time_ms, parse_points(&p.points)?)))
            .collect()
    }

    pub fn background(&self) -> Result<Background, VideoJobError> {
        Background::from_str(&self.background)
    }
}

/// 点の一覧をドメインの `Points` にする（プレビューの `POST /video-jobs/{id}/preview` でも使う）。
pub fn parse_points(points: &[PointRequest]) -> Result<Points, VideoJobError> {
    let points = points
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
