//! `POST /track-visit` ハンドラ。utm 付きの URL で来た訪問を記録する（利用ログ `usage_records` には入れない）。
//!
//! 計測のための呼び出しなので、本文が読めない・`utm_source` が無いなどのときも記録せずに **常に 204** を返す
//! （フロントは結果を見ない。エラーにしても利用者にできることが無い）。
//! 記録はチャネルに投げるだけで重くないので `spawn_blocking` は使わない。

use std::sync::Arc;

use axum::{
    body::Bytes,
    extract::State,
    http::{HeaderMap, StatusCode},
};
use uuid::Uuid;

use crate::{
    analytics::visit_tracking::{event::VisitEvent, visit_recorder::VisitRecorder},
    api::handler::analytics::track_visit::request::TrackVisitRequest,
};

pub async fn track_visit(
    State(visits): State<Arc<dyn VisitRecorder>>,
    headers: HeaderMap,
    body: Bytes,
) -> StatusCode {
    let request: TrackVisitRequest = serde_json::from_slice(&body).unwrap_or_default();
    // 利用ログ（track_usage）と同じく、client-id が無い・不正なら None として記録する
    let client_id = headers
        .get("client-id")
        .and_then(|value| value.to_str().ok())
        .and_then(|value| Uuid::parse_str(value).ok());
    let event = VisitEvent::new(
        client_id,
        request.utm_source.as_deref(),
        request.utm_medium.as_deref(),
        request.utm_campaign.as_deref(),
        request.utm_content.as_deref(),
        request.landing_path.as_deref(),
    );
    if let Some(event) = event {
        visits.record(event);
    }
    StatusCode::NO_CONTENT
}
