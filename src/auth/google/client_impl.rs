use async_trait::async_trait;
use reqwest::{Client, Url};

use crate::{
    auth::google::{client::GoogleAuthClient, config::GoogleLoginConfig, error::GoogleAuthError},
    user::google_identity::GoogleIdentity,
};

const AUTHORIZATION_ENDPOINT: &str = "https://accounts.google.com/o/oauth2/v2/auth";
const TOKEN_ENDPOINT: &str = "https://oauth2.googleapis.com/token";
const USERINFO_ENDPOINT: &str = "https://openidconnect.googleapis.com/v1/userinfo";

#[derive(serde::Deserialize)]
struct TokenResponse {
    access_token: String,
}

#[derive(serde::Deserialize)]
struct UserInfoResponse {
    sub: String,
    email: String,
    #[serde(default)]
    email_verified: bool,
    name: Option<String>,
}

pub struct GoogleAuthClientImpl {
    http: Client,
    config: GoogleLoginConfig,
}

impl GoogleAuthClientImpl {
    pub fn new(config: GoogleLoginConfig) -> Self {
        Self {
            http: Client::new(),
            config,
        }
    }

    async fn exchange_code(&self, code: &str) -> Result<TokenResponse, GoogleAuthError> {
        self.http
            .post(TOKEN_ENDPOINT)
            .form(&[
                ("code", code),
                ("client_id", self.config.client_id()),
                ("client_secret", self.config.client_secret()),
                ("redirect_uri", self.config.redirect_uri()),
                ("grant_type", "authorization_code"),
            ])
            .send()
            .await
            .and_then(|response| response.error_for_status())
            .map_err(|err| GoogleAuthError::Request(err.without_url().to_string()))?
            .json()
            .await
            .map_err(|err| GoogleAuthError::InvalidResponse(err.without_url().to_string()))
    }

    async fn fetch_user_info(
        &self,
        access_token: &str,
    ) -> Result<UserInfoResponse, GoogleAuthError> {
        self.http
            .get(USERINFO_ENDPOINT)
            .bearer_auth(access_token)
            .send()
            .await
            .and_then(|response| response.error_for_status())
            .map_err(|err| GoogleAuthError::Request(err.without_url().to_string()))?
            .json()
            .await
            .map_err(|err| GoogleAuthError::InvalidResponse(err.without_url().to_string()))
    }
}

#[async_trait]
impl GoogleAuthClient for GoogleAuthClientImpl {
    fn authorization_url(&self, state: &str) -> String {
        Url::parse_with_params(
            AUTHORIZATION_ENDPOINT,
            &[
                ("client_id", self.config.client_id()),
                ("redirect_uri", self.config.redirect_uri()),
                ("response_type", "code"),
                ("scope", "openid email profile"),
                ("state", state),
                ("prompt", "select_account"),
            ],
        )
        .expect("Google authorization endpoint must be a valid URL")
        .to_string()
    }

    async fn fetch_identity(&self, code: &str) -> Result<GoogleIdentity, GoogleAuthError> {
        let token = self.exchange_code(code).await?;
        let user_info = self.fetch_user_info(&token.access_token).await?;

        if !user_info.email_verified {
            return Err(GoogleAuthError::EmailNotVerified(user_info.sub));
        }

        Ok(GoogleIdentity::new(
            user_info.sub,
            user_info.email,
            user_info.name,
        ))
    }
}
