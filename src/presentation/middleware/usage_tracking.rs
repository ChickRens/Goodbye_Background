use std::{sync::Arc, time::Instant};

use axum::{
    body::Body,
    extract::{Request, State},
    http::Response,
    middleware::Next,
};
use uuid::Uuid;

use crate::{
    analytics::usage_tracking::{
        event::UsageEvent, id::UsageId, operation::UsageOperation, status::UsageStatus,
        usage_recorder::UsageRecorder,
    },
    composition::wiring::App,
    presentation::middleware::extension::UsageStatusExt,
};

pub async fn track_usage(
    State(app): State<Arc<App>>,
    request: Request,
    next: Next,
) -> Response<Body> {
    let id = request
        .headers()
        .get("client-id")
        .and_then(|value| value.to_str().ok())
        .and_then(|value| Uuid::parse_str(value).ok())
        .map(|value| UsageId::new(value));

    let operation = request.uri().path().trim_start_matches("/").to_string();
    let operation = UsageOperation::new(&operation);
    let start = Instant::now();

    let response = next.run(request).await;

    let end = start.elapsed();

    let status = match response.extensions().get::<UsageStatusExt>() {
        None => UsageStatus::Success,
        Some(err) => UsageStatus::Failed(err.clone().code),
    };

    let event = UsageEvent::new(id, operation, status, end);
    app.usage_recorder().record(event).await;

    response
}
