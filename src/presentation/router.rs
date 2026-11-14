use std::sync::Arc;

use axum::{
    Router,
    extract::DefaultBodyLimit,
    middleware::from_fn_with_state,
    routing::{get, post},
};
use axum::http::{HeaderValue, header::CACHE_CONTROL};
use tower_http::{services::ServeDir, set_header::SetResponseHeader};

use crate::{
    api::handler::image_cutout::{
        get_completed::handler::get_completed_image, get_preview::handler::get_preview,
        redo::handler::redo, save::handler::save, segment::handler::segment, undo::handler::undo,
        upload::handler::upload,
    },
    api::handler::video_cutout::{
        create_job::handler::create_video_job, get_job::handler::get_video_job,
        preview_mask::handler::preview_video_mask, report_event::handler::report_video_job_event,
        set_preview_frame::handler::set_preview_frame, start_job::handler::start_video_job,
    },
    composition::{video_wiring::VideoCutoutApp, wiring::App},
    presentation::middleware::usage_tracking::track_usage,
    video_cutout::set_preview_frame::usecase::MAX_PREVIEW_IMAGE_BYTES,
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
/// - `DefaultBodyLimit` 10MB: アップロード画像の上限。画像のルートと、動画のプレビュー用フレームの登録にだけ掛ける
///   （それ以外の動画の本文は小さい JSON）
/// - `track_usage`: 画像の 7 API と、動画の作成・開始の利用ログを記録する
/// - 動画の取得（ポーリング）・プレビュー・通知は `track_usage` の後に merge するので記録しない
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
        .fallback_service(static_files("frontend"))
}

/// フロントエンドの静的ファイル。`Cache-Control: no-cache` を付ける（2026-11-12）。
/// 付けないとブラウザ（特に iPhone の Safari）が最終更新日時から推測した期間だけ古い JS を使い続け、デプロイ後に
/// 新しい HTML と古い JS、または新旧の JS どうしが混ざって動かなくなる（複数フレームの点のデプロイで実際に起きた）。
/// no-cache は「毎回、変わったかをサーバーに確かめる」意味で、変わっていなければ 304 が返るだけなので重くない。
fn static_files(dir: &str) -> SetResponseHeader<ServeDir, HeaderValue> {
    SetResponseHeader::overriding(
        ServeDir::new(dir).append_index_html_on_directories(true),
        CACHE_CONTROL,
        HeaderValue::from_static("no-cache"),
    )
}

/// 動画切り抜きのルート。（利用ログに記録するもの, 記録しないもの）の 2 つを返す。
///
/// | パス | ハンドラ | 入力 | 出力 | 利用ログ |
/// |---|---|---|---|---|
/// | `POST /video-jobs` | `create_video_job` | `{size_bytes, content_type, start_ms, end_ms}` | 201 `{job_id, upload_url, remaining_today}` | 記録する |
/// | `POST /video-jobs/{id}/start` | `start_video_job` | `{points: [{x, y, label}], background}` | 202 `{phase, failure_code, remaining_today}` | 記録する |
/// | `GET /video-jobs/{id}` | `get_video_job` | なし | 200 `{phase, progress, failure_code, quota_refunded, download_url, remaining_today}` | しない（ポーリング） |
/// | `POST /video-jobs/{id}/preview-frame` | `set_preview_frame` | multipart（`file`。PNG / JPEG、10MB 以下、長辺 2000px 以下） | 204 | しない |
/// | `POST /video-jobs/{id}/preview` | `preview_video_mask` | `{points: [{x, y, label}]}` | 200 `image/png`（グレースケールの alpha。フレームと同じ大きさ）+ ヘッダー `X-Object-Present` | しない |
/// | `POST /internal/video-jobs/{id}/events` | `report_video_job_event` | `WorkerEvent`（HMAC 署名付き） | 204 | しない |
///
/// 利用者向けの 5 つは `client-id` ヘッダー（UUID）が必須（無ければ 400 `MISSING_CLIENT_ID`）。
/// プレビューの 2 つはジョブの持ち主だけ・`created` のときだけ使え、回数を消費しない。
/// IP は `presentation::client_ip`（`TRUSTED_PROXY_HOPS`）で取るので、serve は `into_make_service_with_connect_info` が必要。
pub fn video_routes(video: Arc<VideoCutoutApp>) -> (Router, Router) {
    let tracked = Router::new()
        .route("/video-jobs", post(create_video_job))
        .route("/video-jobs/{id}/start", post(start_video_job))
        .with_state(video.clone());
    let untracked = Router::new()
        .route("/video-jobs/{id}", get(get_video_job))
        .route(
            "/video-jobs/{id}/preview-frame",
            post(set_preview_frame).layer(DefaultBodyLimit::max(MAX_PREVIEW_IMAGE_BYTES)),
        )
        .route("/video-jobs/{id}/preview", post(preview_video_mask))
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
            testing::{FakeStorage, FakeWorker, InMemoryQuota, NOW_MS, png_bytes},
        },
    };

    const SECRET: &str = "test-callback-secret";
    const CLIENT_ID: &str = "11111111-2222-4333-8444-555555555555";

    fn router_for(video: VideoCutoutApp) -> Router {
        router_for_shared(Arc::new(video))
    }

    fn router_for_shared(video: Arc<VideoCutoutApp>) -> Router {
        let (tracked, untracked) = video_routes(video);
        tracked
            .merge(untracked)
            .layer(MockConnectInfo(SocketAddr::from(([203, 0, 113, 7], 40000))))
    }

    fn enabled_router() -> Router {
        router_for(enabled_app())
    }

    fn enabled_app() -> VideoCutoutApp {
        VideoCutoutApp::for_test(
            Arc::new(FakeStorage::new(Some(1_000_000))),
            Arc::new(FakeWorker::new()),
            Arc::new(InMemoryQuota::new()),
            Arc::new(FakeVideoClock::new(NOW_MS)),
            SECRET,
        )
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
        json!({"prompts": [{"time_ms": 1000, "points": [{"x": 0.5, "y": 0.4, "label": "include"}]}], "background": "green"})
    }

    const BOUNDARY: &str = "preview-frame-boundary";

    fn preview_frame_with_client(id: &str, image: &[u8], client_id: &str) -> Request<Body> {
        let mut body = format!(
            "--{BOUNDARY}\r\nContent-Disposition: form-data; name=\"file\"; filename=\"frame.png\"\r\nContent-Type: image/png\r\n\r\n"
        )
        .into_bytes();
        body.extend_from_slice(image);
        body.extend_from_slice(format!("\r\n--{BOUNDARY}--\r\n").as_bytes());
        Request::post(format!("/video-jobs/{id}/preview-frame?time_ms=1000"))
            .header("client-id", client_id)
            .header("content-type", format!("multipart/form-data; boundary={BOUNDARY}"))
            .body(Body::from(body))
            .unwrap()
    }

    fn preview_frame(id: &str, image: &[u8]) -> Request<Body> {
        preview_frame_with_client(id, image, CLIENT_ID)
    }

    fn preview_body() -> Value {
        json!({"time_ms": 1000, "points": [{"x": 0.25, "y": 0.5, "label": "include"}, {"x": 0.75, "y": 0.5, "label": "exclude"}]})
    }

    async fn created_job(router: &Router) -> String {
        let (status, created) = send(router, post_json("/video-jobs", create_body())).await;
        assert_eq!(status, StatusCode::CREATED);
        created["job_id"].as_str().unwrap().to_string()
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
    async fn static_files_are_revalidated_every_time() {
        // 古い JS が使われ続けないよう、静的ファイルには no-cache を付ける
        let response = super::static_files("frontend")
            .oneshot(Request::get("/video.html").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response.headers()["cache-control"], "no-cache");
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
        let (status, _) = send(&router, preview_frame(id, &png_bytes(4, 4))).await;
        assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
        let (status, _) = send(&router, post_json(&format!("/video-jobs/{id}/preview"), preview_body())).await;
        assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
    }

    #[tokio::test]
    async fn preview_frame_without_valid_time_ms_is_400() {
        let router = enabled_router();
        let id = created_job(&router).await;
        for query in ["", "?time_ms=abc", "?time_ms=-5"] {
            let mut request = preview_frame(&id, &png_bytes(4, 4));
            *request.uri_mut() = format!("/video-jobs/{id}/preview-frame{query}").parse().unwrap();
            let (status, body) = send(&router, request).await;
            assert_eq!(status, StatusCode::BAD_REQUEST, "{query}");
            assert_eq!(body, json!({"code": "INVALID_PREVIEW_TIME"}), "{query}");
        }
    }

    #[tokio::test]
    async fn start_with_old_points_body_is_400() {
        let router = enabled_router();
        let id = created_job(&router).await;
        let old = json!({"points": [{"x": 0.5, "y": 0.4, "label": "include"}], "background": "green"});
        let (status, body) = send(&router, post_json(&format!("/video-jobs/{id}/start"), old)).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(body, json!({"code": "INVALID_REQUEST_BODY"}));
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

        let body = json!({"prompts": [{"time_ms": 1000, "points": [{"x": 0.5, "y": 0.4, "label": "maybe"}]}], "background": "green"});
        let (status, got) = send(&router, post_json(&uri, body)).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(got, json!({"code": "INVALID_POINTS"}));

        let body = json!({"prompts": [{"time_ms": 1000, "points": [{"x": 0.5, "y": 0.4, "label": "include"}]}], "background": "red"});
        let (status, got) = send(&router, post_json(&uri, body)).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(got, json!({"code": "INVALID_BACKGROUND"}));

        // 1 つ目のフレームが範囲の開始（1000）でない
        let body = json!({"prompts": [{"time_ms": 2000, "points": [{"x": 0.5, "y": 0.4, "label": "include"}]}], "background": "green"});
        let (status, got) = send(&router, post_json(&uri, body)).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(got, json!({"code": "INVALID_PROMPTS"}));
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

    #[tokio::test]
    async fn preview_frame_then_preview_returns_png_without_consuming_quota() {
        let router = enabled_router();
        let id = created_job(&router).await;
        let (status, body) = send(&router, preview_frame(&id, &png_bytes(40, 30))).await;
        assert_eq!(status, StatusCode::NO_CONTENT);
        assert_eq!(body, Value::Null);

        let response = router
            .clone()
            .oneshot(post_json(&format!("/video-jobs/{id}/preview"), preview_body()))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response.headers()["content-type"], "image/png");
        assert_eq!(response.headers()["x-object-present"], "true");
        let bytes = axum::body::to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let mask = image::load_from_memory_with_format(&bytes, image::ImageFormat::Png).unwrap();
        assert_eq!(mask.color(), image::ColorType::L8);
        let mask = mask.to_luma8();
        assert_eq!(mask.dimensions(), (40, 30));
        assert_eq!(mask.get_pixel(0, 0).0, [255]);
        assert_eq!(mask.get_pixel(39, 0).0, [0]);

        let (_, got) = send(&router, get(&format!("/video-jobs/{id}"))).await;
        assert_eq!(got["phase"], json!("created"));
        assert_eq!(got["remaining_today"], json!(3));
    }

    #[tokio::test]
    async fn preview_without_frame_is_409() {
        let router = enabled_router();
        let id = created_job(&router).await;
        let (status, body) =
            send(&router, post_json(&format!("/video-jobs/{id}/preview"), preview_body())).await;
        assert_eq!(status, StatusCode::CONFLICT);
        assert_eq!(body, json!({"code": "PREVIEW_FRAME_NOT_FOUND"}));
    }

    #[tokio::test]
    async fn preview_after_start_is_409_invalid_state() {
        let router = enabled_router();
        let id = created_job(&router).await;
        let (status, _) = send(&router, preview_frame(&id, &png_bytes(8, 8))).await;
        assert_eq!(status, StatusCode::NO_CONTENT);
        let (status, _) = send(&router, post_json(&format!("/video-jobs/{id}/start"), start_body())).await;
        assert_eq!(status, StatusCode::ACCEPTED);

        let (status, body) =
            send(&router, post_json(&format!("/video-jobs/{id}/preview"), preview_body())).await;
        assert_eq!(status, StatusCode::CONFLICT);
        assert_eq!(body, json!({"code": "VIDEO_JOB_INVALID_STATE"}));
        let (status, body) = send(&router, preview_frame(&id, &png_bytes(8, 8))).await;
        assert_eq!(status, StatusCode::CONFLICT);
        assert_eq!(body, json!({"code": "VIDEO_JOB_INVALID_STATE"}));
    }

    #[tokio::test]
    async fn preview_of_other_clients_job_is_404() {
        let router = enabled_router();
        let id = created_job(&router).await;
        let (status, _) = send(&router, preview_frame(&id, &png_bytes(8, 8))).await;
        assert_eq!(status, StatusCode::NO_CONTENT);

        let other = "99999999-2222-4333-8444-555555555555";
        let (status, body) = send(&router, preview_frame_with_client(&id, &png_bytes(8, 8), other)).await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        assert_eq!(body, json!({"code": "VIDEO_JOB_NOT_FOUND"}));
        let request = Request::post(format!("/video-jobs/{id}/preview"))
            .header("client-id", other)
            .header("content-type", "application/json")
            .body(Body::from(preview_body().to_string()))
            .unwrap();
        let (status, body) = send(&router, request).await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        assert_eq!(body, json!({"code": "VIDEO_JOB_NOT_FOUND"}));
    }

    #[tokio::test]
    async fn preview_rejects_zero_or_too_many_points() {
        let router = enabled_router();
        let id = created_job(&router).await;
        let (status, _) = send(&router, preview_frame(&id, &png_bytes(8, 8))).await;
        assert_eq!(status, StatusCode::NO_CONTENT);
        let uri = format!("/video-jobs/{id}/preview");
        let nine: Vec<Value> = (0..9).map(|_| json!({"x": 0.5, "y": 0.5, "label": "include"})).collect();
        for body in [json!({"time_ms": 1000, "points": []}), json!({"time_ms": 1000, "points": nine})] {
            let (status, got) = send(&router, post_json(&uri, body)).await;
            assert_eq!(status, StatusCode::BAD_REQUEST);
            assert_eq!(got, json!({"code": "INVALID_POINTS"}));
        }
    }

    #[tokio::test]
    async fn preview_frame_rejects_too_large_images() {
        let router = enabled_router();
        let id = created_job(&router).await;
        // 長辺が 2000px を超える
        let (status, body) = send(&router, preview_frame(&id, &png_bytes(2001, 4))).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(body, json!({"code": "INVALID_PREVIEW_IMAGE"}));
        // 本文が 10MB を超える
        let huge = vec![0u8; 10 * 1024 * 1024 + 1];
        let (status, body) = send(&router, preview_frame(&id, &huge)).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(body, json!({"code": "INVALID_PREVIEW_IMAGE"}));
        // 画像ではない
        let (status, body) = send(&router, preview_frame(&id, b"hello")).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(body, json!({"code": "INVALID_PREVIEW_IMAGE"}));
    }

    #[tokio::test]
    async fn preview_frame_accepts_a_few_megabytes() {
        // axum の既定の本文の上限（2MB）より大きい正しい画像が通る（ルートの 10MB の上限が効いている）
        let router = enabled_router();
        let id = created_job(&router).await;
        let mut state: u32 = 12345;
        let noise = image::RgbImage::from_fn(1200, 1200, |_, _| {
            state = state.wrapping_mul(1_103_515_245).wrapping_add(12_345);
            let b = (state >> 8).to_le_bytes();
            image::Rgb([b[0], b[1], b[2]])
        });
        let mut png = Vec::new();
        noise
            .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
            .unwrap();
        assert!((3 * 1024 * 1024..=5 * 1024 * 1024).contains(&png.len()), "{}", png.len());
        let (status, _) = send(&router, preview_frame(&id, &png)).await;
        assert_eq!(status, StatusCode::NO_CONTENT);
    }

    #[tokio::test]
    async fn preview_frame_is_503_when_encodes_are_busy() {
        let video = Arc::new(enabled_app());
        let router = router_for_shared(video.clone());
        let id = created_job(&router).await;
        let held: Vec<_> = (0..crate::composition::video_wiring::MAX_CONCURRENT_PREVIEW_ENCODES)
            .map(|_| video.try_acquire_preview_encode().unwrap())
            .collect();
        let (status, body) = send(&router, preview_frame(&id, &png_bytes(8, 8))).await;
        assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(body, json!({"code": "PREVIEW_BUSY"}));
        // 空けば通る
        drop(held);
        let (status, _) = send(&router, preview_frame(&id, &png_bytes(8, 8))).await;
        assert_eq!(status, StatusCode::NO_CONTENT);
    }
}
