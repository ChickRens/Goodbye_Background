use async_trait::async_trait;

use crate::{
    auth::login_session::repository::error::LoginSessionRepositoryError,
    user::{id::UserId, user::User},
};

#[async_trait]
pub trait LoginSessionRepository: Send + Sync {
    async fn create(
        &self,
        token_hash: &str,
        user_id: &UserId,
        expires_at: i64,
    ) -> Result<(), LoginSessionRepositoryError>;

    /// 有効期限内のセッションに紐づくユーザーを返す。
    async fn find_user(
        &self,
        token_hash: &str,
        now: i64,
    ) -> Result<Option<User>, LoginSessionRepositoryError>;

    async fn delete(&self, token_hash: &str) -> Result<(), LoginSessionRepositoryError>;

    async fn delete_expired(&self, now: i64) -> Result<(), LoginSessionRepositoryError>;
}
