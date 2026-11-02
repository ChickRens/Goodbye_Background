use std::sync::Arc;

use axum::{
    Router,
    extract::DefaultBodyLimit,
    middleware::from_fn_with_state,
    routing::{delete, get, post},
};
use tower_http::services::ServeDir;

use crate::{
    api::handler::{
        auth::handler::{delete_account, google_callback, google_login, logout, me},
        get_completed::handler::get_completed_image,
        get_preview::handler::get_preview,
        redo::handler::redo, save::handler::save, segment::handler::segment, undo::handler::undo,
        upload::handler::upload,
    },
    composition::wiring::App,
    presentation::middleware::usage_tracking::track_usage,
};

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
        // 画像編集の利用統計に混ざらないよう、track_usage の外に置く
        .route("/auth/google/login", get(google_login))
        .route("/auth/google/callback", get(google_callback))
        .route("/auth/me", get(me))
        .route("/auth/logout", post(logout))
        .route("/auth/account", delete(delete_account))
        .fallback_service(ServeDir::new("frontend").append_index_html_on_directories(true))
        .with_state(app)
}
