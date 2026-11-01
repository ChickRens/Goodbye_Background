use async_trait::async_trait;

use crate::{auth::google::error::GoogleAuthError, user::google_identity::GoogleIdentity};

#[async_trait]
pub trait GoogleAuthClient: Send + Sync {
    fn authorization_url(&self, state: &str) -> String;
    async fn fetch_identity(&self, code: &str) -> Result<GoogleIdentity, GoogleAuthError>;
}
