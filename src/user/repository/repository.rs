use async_trait::async_trait;

use crate::user::{
    google_identity::GoogleIdentity, repository::error::UserRepositoryError, user::User,
};

#[async_trait]
pub trait UserRepository: Send + Sync {
    /// 初回ログインならユーザーを作り、2回目以降ならemail等を最新の値に更新する。
    async fn upsert_by_google_identity(
        &self,
        identity: &GoogleIdentity,
    ) -> Result<User, UserRepositoryError>;
}
