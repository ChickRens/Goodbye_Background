//! `POST /track-visit` の本文。どれも省略できる（足りない・おかしい値は `VisitEvent::new` で整える）。

use serde::Deserialize;

#[derive(Debug, Default, Deserialize)]
pub struct TrackVisitRequest {
    pub utm_source: Option<String>,
    pub utm_medium: Option<String>,
    pub utm_campaign: Option<String>,
    pub utm_content: Option<String>,
    pub landing_path: Option<String>,
}
