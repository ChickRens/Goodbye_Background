use std::env;

#[derive(Clone)]
pub struct GoogleLoginConfig {
    client_id: String,
    client_secret: String,
    redirect_uri: String,
}

impl GoogleLoginConfig {
    /// composeは未設定の変数を空文字で渡すので、空文字も未設定として扱う。
    pub fn from_env() -> Option<Self> {
        let client_id = non_empty_env("GOOGLE_CLIENT_ID")?;
        let client_secret = non_empty_env("GOOGLE_CLIENT_SECRET")?;
        let base_url = non_empty_env("PUBLIC_BASE_URL")?;

        Some(Self {
            client_id,
            client_secret,
            redirect_uri: format!("{}/auth/google/callback", base_url.trim_end_matches('/')),
        })
    }

    pub fn client_id(&self) -> &str {
        &self.client_id
    }

    pub fn client_secret(&self) -> &str {
        &self.client_secret
    }

    pub fn redirect_uri(&self) -> &str {
        &self.redirect_uri
    }
}

/// httpsで公開しているときはCookieにSecureを付ける。localhostでの開発ではhttpなので付けない。
pub fn uses_secure_cookies() -> bool {
    non_empty_env("PUBLIC_BASE_URL").is_some_and(|url| url.starts_with("https://"))
}

fn non_empty_env(name: &str) -> Option<String> {
    env::var(name).ok().filter(|value| !value.is_empty())
}
