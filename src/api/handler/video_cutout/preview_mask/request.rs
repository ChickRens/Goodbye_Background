//! `POST /video-jobs/{id}/preview` のリクエスト。
//!
//! ```json
//! { "points": [{ "x": 0.5, "y": 0.4, "label": "include" }] }
//! ```
//! 点の形と検証は `start` と同じ（`start_job::request::parse_points`。1〜8 個、0.0〜1.0、`include` / `exclude`）。

use crate::{
    api::handler::video_cutout::start_job::request::{PointRequest, parse_points},
    video_cutout::job::{error::VideoJobError, point::Points},
};

#[derive(serde::Deserialize)]
pub struct PreviewMaskRequest {
    pub points: Vec<PointRequest>,
}

impl PreviewMaskRequest {
    pub fn points(&self) -> Result<Points, VideoJobError> {
        parse_points(&self.points)
    }
}
