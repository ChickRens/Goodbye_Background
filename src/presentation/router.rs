use std::sync::Arc;

use axum::{
    Router,
    extract::DefaultBodyLimit,
    middleware::from_fn_with_state,
    routing::{get, post},
};
use tower_http::services::ServeDir;

use crate::{
    api::handler::image_cutout::{
        get_completed::handler::get_completed_image, get_preview::handler::get_preview,
        redo::handler::redo, save::handler::save, segment::handler::segment, undo::handler::undo,
        upload::handler::upload,
    },
    api::handler::video_cutout::{
        create_job::handler::create_video_job, get_job::handler::get_video_job,
        report_event::handler::report_video_job_event, start_job::handler::start_video_job,
    },
    composition::{video_wiring::VideoCutoutApp, wiring::App},
    presentation::middleware::usage_tracking::track_usage,
};

/// ルーティング定義。画像の API は全て POST（GET の `/get-preview` などもボディに JSON を取るため POST）。
///
/// | パス | ハンドラ | 入力 | 出力 |
/// |---|---|---|---|
/// | `/upload` | `upload` | multipart（フィールド名 `file`） | `{session_id, image_id}` |
/// | `/get-preview` | `get_preview` | `{image_id}` | `image/webp` バイナリ |
/// | `/segment` | `segment` | `{session_id, x, y, is_foreground}` | `{image_id}` |
/// | `/undo` | `undo` | `{session_id}` | `{image_id}` |
/// | `/redo` | `redo` | `{session_id}` | `{image_id}` |
/// | `/save` | `save` | `{session_id}` | `{image_id}`（`/get-completed` 用） |
/// | `/get-completed` | `get_completed_image` | `{image_id}` | `image/webp` バイナリ |
///
/// 動画の API は `video_routes` を参照。
///
/// エラー時は `{ "code": "..." }` の JSON（`AppError::into_response`）。
///
/// レイヤーの注意点（axum の `.layer` は「それ以前に追加した route」にだけ掛かる）:
/// - `DefaultBodyLimit` 10MB: アップロード画像の上限。画像のルートにだけ掛ける（動画の本文は小さい JSON）
/// - `track_usage`: 画像の 7 API と、動画の作成・開始の利用ログを記録する
/// - 動画の取得（ポーリング）と通知は `track_usage` の後に merge するので記録しない
/// - `fallback_service`: それ以外のパスは `frontend/` ディレクトリを静的配信（相対パス。cwd 依存）
pub fn route(app: Arc<App>, video: Arc<VideoCutoutApp>) -> Router {
    let (video_tracked, video_untracked) = video_routes(video);
    Router::new()
        .route("/upload", post(upload))
        .route("/get-preview", post(get_preview))
        .route("/segment", post(segment))
        .route("/undo", post(undo))
        .route("/redo", post(redo))
        .route("/get-completed", post(get_completed_image))
        .route("/save", post(save))
        .layer(DefaultBodyLimit::max(10 * 1024 * 1024))
        // 画像と動画で State が違うので、先に画像側の State を確定させてから merge する
        .with_state(app.clone())
        .merge(video_tracked)
        .layer(from_fn_with_state(app, track_usage))
        .merge(video_untracked)
        .fallback_service(ServeDir::new("frontend").append_index_html_on_directories(true))
}

/// 動画切り抜きのルート。（利用ログに記録するもの, 記録しないもの）の 2 つを返す。
///
/// | パス | ハンドラ | 入力 | 出力 | 利用ログ |
/// |---|---|---|---|---|
/// | `POST /video-jobs` | `create_video_job` | `{size_bytes, content_type, start_ms, end_ms}` | 201 `{job_id, upload_url, remaining_today}` | 記録する |
/// | `POST /video-jobs/{id}/start` | `start_video_job` | `{points: [{x, y, label}], background}` | 202 `{phase, failure_code, remaining_today}` | 記録する |
/// | `GET /video-jobs/{id}` | `get_video_job` | なし | 200 `{phase, progress, failure_code, quota_refunded, download_url, remaining_today}` | しない（ポーリング） |
/// | `POST /internal/video-jobs/{id}/events` | `report_video_job_event` | `WorkerEvent`（HMAC 署名付き） | 204 | しない |
///
/// 利用者向けの 3 つは `client-id` ヘッダー（UUID）が必須（無ければ 400 `MISSING_CLIENT_ID`）。
/// IP は `presentation::client_ip`（`TRUSTED_PROXY_HOPS`）で取るので、serve は `into_make_service_with_connect_info` が必要。
pub fn video_routes(video: Arc<VideoCutoutApp>) -> (Router, Router) {
    let tracked = Router::new()
        .route("/video-jobs", post(create_video_job))
        .route("/video-jobs/{id}/start", post(start_video_job))
        .with_state(video.clone());
    let untracked = Router::new()
        .route("/video-jobs/{id}", get(get_video_job))
        .route("/internal/video-jobs/{id}/events", post(report_video_job_event))
        .with_state(video);
    (tracked, untracked)
}

#[cfg(test)]
mod router_test {
    use std::{net::SocketAddr, sync::Arc};

    use axum::{
        Router,
        body::Body,
        extract::connect_info::MockConnectInfo,
        http::{Request, StatusCode},
    };
    use serde_json::{Value, json};
    use tower::ServiceExt;

    use super::video_routes;
    use crate::{
        composition::video_wiring::VideoCutoutApp,
        video_cutout::{
            clock_impl::FakeVideoClock,
            signature::sign,
            testing::{FakeStorage, FakeWorker, InMemoryQuota, NOW_MS},
        },
    };

    const SECRET: &str = "test-callback-secret";
    const CLIENT_ID: &str = "11111111-2222-4333-8444-555555555555";

    fn router_for(video: VideoCutoutApp) -> Router {
        let (tracked, untracked) = video_routes(Arc::new(video));
        tracked
            .merge(untracked)
            .layer(MockConnectInfo(SocketAddr::from(([203, 0, 113, 7], 40000))))
    }

    fn enabled_router() -> Router {
        router_for(VideoCutoutApp::for_test(
            Arc::new(FakeStorage::new(Some(1_000_000))),
            Arc::new(FakeWorker::new()),
            Arc::new(InMemoryQuota::new()),
            Arc::new(FakeVideoClock::new(NOW_MS)),
            SECRET,
        ))
    }

    fn disabled_router() -> Router {
        router_for(VideoCutoutApp::new(Arc::new(InMemoryQuota::new()), None))
    }

    async fn send(router: &Router, request: Request<Body>) -> (StatusCode, Value) {
        let response = router.clone().oneshot(request).await.unwrap();
        let status = response.status();
        let bytes = axum::body::to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let body = if bytes.is_empty() {
            Value::Null
        } else {
            serde_json::from_slice(&bytes).unwrap()
        };
        (status, body)
    }

    fn post_json(uri: &str, body: Value) -> Request<Body> {
        Request::post(uri)
            .header("client-id", CLIENT_ID)
            .header("content-type", "application/json")
            .body(Body::from(body.to_string()))
            .unwrap()
    }

    fn get(uri: &str) -> Request<Body> {
        Request::get(uri).header("client-id", CLIENT_ID).body(Body::empty()).unwrap()
    }

    fn event(id: &str, body: &Value, signature: Option<String>) -> Request<Body> {
        let body = body.to_string();
        let ts = NOW_MS / 1000;
        let signature = signature.unwrap_or_else(|| sign(SECRET.as_bytes(), ts, body.as_bytes()));
        Request::post(format!("/internal/video-jobs/{id}/events"))
            .header("content-type", "application/json")
            .header("x-signature-timestamp", ts.to_string())
            .header("x-signature", signature)
            .body(Body::from(body))
            .unwrap()
    }

    fn create_body() -> Value {
        json!({"size_bytes": 10_000_000, "content_type": "video/mp4", "start_ms": 1000, "end_ms": 9000})
    }

    fn start_body() -> Value {
        json!({"points": [{"x": 0.5, "y": 0.4, "label": "include"}], "background": "green"})
    }

    async fn started_job(router: &Router) -> String {
        let (status, created) = send(router, post_json("/video-jobs", create_body())).await;
        assert_eq!(status, StatusCode::CREATED);
        let id = created["job_id"].as_str().unwrap().to_string();
        let (status, _) = send(router, post_json(&format!("/video-jobs/{id}/start"), start_body())).await;
        assert_eq!(status, StatusCode::ACCEPTED);
        id
    }

    #[tokio::test]
    async fn video_routes_return_503_when_disabled() {
        let router = disabled_router();
        let (status, body) = send(&router, post_json("/video-jobs", json!({}))).await;
        assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(body, json!({"code": "VIDEO_CUTOUT_DISABLED"}));

        let id = "6f1c2b9e-0d4a-4c1e-9a57-3f2e8b1d7c60";
        let (status, _) = send(&router, get(&format!("/video-jobs/{id}"))).await;
        assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
        let (status, _) =
            send(&router, post_json(&format!("/video-jobs/{id}/start"), json!({}))).await;
        assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
        let (status, _) =
            send(&router, event(id, &json!({}), Some("00".to_string()))).await;
        assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
    }

    #[tokio::test]
    async fn missing_or_invalid_client_id_is_400() {
        let router = enabled_router();
        let request = Request::post("/video-jobs")
            .header("content-type", "application/json")
            .body(Body::from(create_body().to_string()))
            .unwrap();
        let (status, body) = send(&router, request).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(body, json!({"code": "MISSING_CLIENT_ID"}));

        let request = Request::get("/video-jobs/6f1c2b9e-0d4a-4c1e-9a57-3f2e8b1d7c60")
            .header("client-id", "not-a-uuid")
            .body(Body::empty())
            .unwrap();
        let (status, body) = send(&router, request).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(body, json!({"code": "MISSING_CLIENT_ID"}));
    }

    #[tokio::test]
    async fn create_start_get_return_documented_shapes() {
        let router = enabled_router();
        let (status, created) = send(&router, post_json("/video-jobs", create_body())).await;
        assert_eq!(status, StatusCode::CREATED);
        assert!(created["job_id"].is_string());
        assert!(created["upload_url"].as_str().unwrap().starts_with("https://fake.r2/"));
        assert_eq!(created["remaining_today"], json!(3));

        let id = created["job_id"].as_str().unwrap();
        let (status, started) =
            send(&router, post_json(&format!("/video-jobs/{id}/start"), start_body())).await;
        assert_eq!(status, StatusCode::ACCEPTED);
        assert_eq!(started, json!({"phase": "queued", "failure_code": null, "remaining_today": 2}));

        let (status, got) = send(&router, get(&format!("/video-jobs/{id}"))).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(
            got,
            json!({
                "phase": "queued", "progress": null, "failure_code": null,
                "quota_refunded": false, "download_url": null, "remaining_today": 2
            })
        );
    }

    #[tokio::test]
    async fn start_rejects_unknown_label_and_background() {
        let router = enabled_router();
        let (_, created) = send(&router, post_json("/video-jobs", create_body())).await;
        let id = created["job_id"].as_str().unwrap();
        let uri = format!("/video-jobs/{id}/start");

        let body = json!({"points": [{"x": 0.5, "y": 0.4, "label": "maybe"}], "background": "green"});
        let (status, got) = send(&router, post_json(&uri, body)).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(got, json!({"code": "INVALID_POINTS"}));

        let body = json!({"points": [{"x": 0.5, "y": 0.4, "label": "include"}], "background": "red"});
        let (status, got) = send(&router, post_json(&uri, body)).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(got, json!({"code": "INVALID_BACKGROUND"}));
    }

    #[tokio::test]
    async fn events_endpoint_rejects_bad_signature_and_mismatched_id() {
        let router = enabled_router();
        let id = started_job(&router).await;
        let body = json!({"job_id": id, "phase": "tracking", "progress": 0.25});

        // 署名なし → 401
        let request = Request::post(format!("/internal/video-jobs/{id}/events"))
            .header("content-type", "application/json")
            .body(Body::from(body.to_string()))
            .unwrap();
        let (status, got) = send(&router, request).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);
        assert_eq!(got, json!({"code": "INVALID_SIGNATURE"}));

        // 不正な署名 → 401
        let (status, _) = send(&router, event(&id, &body, Some("ab".repeat(32)))).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);

        // 正しい署名で別の ID → 400
        let other = "6f1c2b9e-0d4a-4c1e-9a57-3f2e8b1d7c60";
        let (status, got) = send(&router, event(other, &body, None)).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(got, json!({"code": "JOB_ID_MISMATCH"}));

        // 正しい → 204、状態が進む
        let (status, got) = send(&router, event(&id, &body, None)).await;
        assert_eq!(status, StatusCode::NO_CONTENT);
        assert_eq!(got, Value::Null);
        let (_, got) = send(&router, get(&format!("/video-jobs/{id}"))).await;
        assert_eq!(got["phase"], json!("tracking"));
        assert_eq!(got["progress"], json!(0.25));
    }

    #[tokio::test]
    async fn failed_job_reports_failure_code() {
        let router = enabled_router();
        let id = started_job(&router).await;
        let body = json!({"job_id": id, "phase": "failed", "code": "subject_not_found"});
        let (status, _) = send(&router, event(&id, &body, None)).await;
        assert_eq!(status, StatusCode::NO_CONTENT);

        let (_, got) = send(&router, get(&format!("/video-jobs/{id}"))).await;
        assert_eq!(got["phase"], json!("failed"));
        assert_eq!(got["failure_code"], json!("subject_not_found"));
        assert_eq!(got["quota_refunded"], json!(true));
        assert_eq!(got["remaining_today"], json!(3));
    }
}
