use std::sync::Arc;

use axum::{Router, extract::DefaultBodyLimit, middleware::from_fn_with_state, routing::post};
use tower_http::services::ServeDir;

use crate::{
    api::handler::{
        get_completed::handler::get_completed_image, get_preview::handler::get_preview,
        redo::handler::redo, save::handler::save, segment::handler::segment, undo::handler::undo,
        upload::handler::upload,
    },
    composition::wiring::App,
    presentation::middleware::usage_tracking::track_usage,
};

/// ルーティング定義。全 API は POST（GET の `/get-preview` などもボディに JSON を取るため POST）。
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
/// エラー時は `{ "code": "..." }` の JSON（`AppError::into_response`）。
///
/// レイヤーの注意点（axum の `.layer` は「それ以前に追加した route」にだけ掛かる）:
/// - `DefaultBodyLimit` 10MB: アップロード画像の上限。フロント側でも同じ値を前提にしている
/// - `track_usage`: 上の 7 API の利用ログを記録する
/// - `fallback_service`: それ以外のパスは `frontend/` ディレクトリを静的配信（相対パス。cwd 依存）
pub fn route(app: Arc<App>) -> Router {
    Router::new()
        .route("/upload", post(upload))
        .route("/get-preview", post(get_preview))
        .route("/segment", post(segment))
        .route("/undo", post(undo))
        .route("/redo", post(redo))
        .route("/get-completed", post(get_completed_image))
        .route("/save", post(save))
        .layer(DefaultBodyLimit::max(10 * 1024 * 1024))
        .layer(from_fn_with_state(app.clone(), track_usage))
        .fallback_service(ServeDir::new("frontend").append_index_html_on_directories(true))
        .with_state(app)
}
