//! 全 API リクエストの利用ログを記録するミドルウェア。
//!
//! 記録内容: `client-id` ヘッダー（UUID）/ 呼ばれたパス / 処理時間 / 成功・失敗（エラーコード）。
//! 成否は「レスポンス拡張に `UsageStatusExt` があるか」で判定する（`AppError::into_response` が入れる）。
//! そのため axum 自体が返すエラー（JSON パース失敗の 422、ボディサイズ超過の 413 など）は
//! `AppError` を経由しないので **成功として記録される** 点に注意。

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

    // "/segment" → "segment" → UsageOperation::Segment
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
    // チャネルに投げるだけなのでレスポンスは遅れない（INSERT は別タスク）。
    app.usage_recorder().record(event);

    response
}
