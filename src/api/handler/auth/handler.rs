use std::{sync::Arc, time::Duration};

use axum::{
    Json,
    extract::{Query, State},
    http::{StatusCode, header},
    response::{IntoResponse, Redirect},
};
use axum_extra::extract::{
    CookieJar,
    cookie::{Cookie, SameSite},
};
use cookie::time;

use crate::{
    api::handler::auth::response::CurrentUserResponse,
    auth::token::OpaqueToken,
    composition::wiring::App,
    config::{LOGIN_SESSION_LIFETIME, OAUTH_STATE_LIFETIME},
    presentation::errors::app_error::AppError,
};

const SESSION_COOKIE: &str = "gb_session";
const OAUTH_STATE_COOKIE: &str = "gb_oauth_state";
const OAUTH_STATE_COOKIE_PATH: &str = "/auth/google";

const AFTER_LOGIN_PATH: &str = "/app.html";
const LOGIN_FAILED_PATH: &str = "/app.html?login=failed";

#[derive(serde::Deserialize)]
pub struct GoogleCallbackQuery {
    code: Option<String>,
    state: Option<String>,
    error: Option<String>,
}

pub async fn google_login(State(app): State<Arc<App>>, jar: CookieJar) -> impl IntoResponse {
    match app.start_login() {
        Ok(start) => {
            let cookie = auth_cookie(
                OAUTH_STATE_COOKIE,
                start.state.value().to_string(),
                OAUTH_STATE_COOKIE_PATH,
                app.uses_secure_cookies(),
                OAUTH_STATE_LIFETIME,
            );

            (jar.add(cookie), Redirect::to(&start.authorization_url))
        }
        Err(err) => {
            eprintln!("Failed to start login: {err:?}");
            (jar, Redirect::to(LOGIN_FAILED_PATH))
        }
    }
}

pub async fn google_callback(
    State(app): State<Arc<App>>,
    Query(query): Query<GoogleCallbackQuery>,
    jar: CookieJar,
) -> impl IntoResponse {
    let expected_state = jar
        .get(OAUTH_STATE_COOKIE)
        .map(|cookie| cookie.value().to_string());
    let jar = jar.remove(removal_cookie(OAUTH_STATE_COOKIE, OAUTH_STATE_COOKIE_PATH));

    // ユーザーがGoogleの同意画面でキャンセルした場合は、失敗として通知しない
    if query.error.is_some() {
        return (jar, Redirect::to(AFTER_LOGIN_PATH));
    }

    let code = match (query.code, query.state, expected_state) {
        (Some(code), Some(state), Some(expected)) if state == expected => code,
        _ => {
            eprintln!("Rejected login callback: invalid OAuth state");
            return (jar, Redirect::to(LOGIN_FAILED_PATH));
        }
    };

    match app.complete_login(&code).await {
        Ok(token) => {
            let cookie = auth_cookie(
                SESSION_COOKIE,
                token.value().to_string(),
                "/",
                app.uses_secure_cookies(),
                LOGIN_SESSION_LIFETIME,
            );

            (jar.add(cookie), Redirect::to(AFTER_LOGIN_PATH))
        }
        Err(err) => {
            eprintln!("Failed to complete login: {err:?}");
            (jar, Redirect::to(LOGIN_FAILED_PATH))
        }
    }
}

pub async fn me(
    State(app): State<Arc<App>>,
    jar: CookieJar,
) -> Result<impl IntoResponse, AppError> {
    let user = match session_token(&jar) {
        Some(token) => app.current_user(&token).await?,
        None => None,
    };

    Ok((
        [(header::CACHE_CONTROL, "no-store")],
        Json(CurrentUserResponse::new(user)),
    ))
}

pub async fn logout(
    State(app): State<Arc<App>>,
    jar: CookieJar,
) -> Result<impl IntoResponse, AppError> {
    if let Some(token) = session_token(&jar) {
        app.logout(&token).await?;
    }

    Ok((
        jar.remove(removal_cookie(SESSION_COOKIE, "/")),
        StatusCode::NO_CONTENT,
    ))
}

fn session_token(jar: &CookieJar) -> Option<OpaqueToken> {
    jar.get(SESSION_COOKIE)
        .map(|cookie| OpaqueToken::from_value(cookie.value().to_string()))
}

// SameSite=StrictだとGoogleから戻ったときにCookieが送られないのでLaxにする
fn auth_cookie(
    name: &'static str,
    value: String,
    path: &'static str,
    secure: bool,
    lifetime: Duration,
) -> Cookie<'static> {
    Cookie::build((name, value))
        .path(path)
        .http_only(true)
        .same_site(SameSite::Lax)
        .secure(secure)
        .max_age(time::Duration::seconds(lifetime.as_secs() as i64))
        .build()
}

fn removal_cookie(name: &'static str, path: &'static str) -> Cookie<'static> {
    Cookie::build(name).path(path).build()
}
