use async_trait::async_trait;
use sqlx::{PgPool, SqlitePool};
use uuid::Uuid;

use crate::user::{
    google_identity::GoogleIdentity,
    id::UserId,
    repository::{error::UserRepositoryError, repository::UserRepository},
    user::User,
};

const UPSERT_BY_GOOGLE_IDENTITY: &str = "
    INSERT INTO users (id, google_sub, email, display_name)
    VALUES ($1, $2, $3, $4)
    ON CONFLICT (google_sub) DO UPDATE SET
        email = excluded.email,
        display_name = excluded.display_name,
        updated_at = CURRENT_TIMESTAMP
    RETURNING id, email, display_name
";

pub type UserRow = (String, String, Option<String>);

pub fn user_from_row((id, email, display_name): UserRow) -> Result<User, uuid::Error> {
    let id = UserId::from_uuid(Uuid::parse_str(&id)?);
    Ok(User::new(id, email, display_name))
}

// SQLはSQLite/PostgreSQL共通なので、型の違うpoolごとの実装はマクロで生成する。
macro_rules! impl_user_repository {
    ($name:ident, $pool:ty) => {
        pub struct $name {
            pool: $pool,
        }

        impl $name {
            pub fn new(pool: $pool) -> Self {
                Self { pool }
            }
        }

        #[async_trait]
        impl UserRepository for $name {
            async fn upsert_by_google_identity(
                &self,
                identity: &GoogleIdentity,
            ) -> Result<User, UserRepositoryError> {
                let row: UserRow = sqlx::query_as(UPSERT_BY_GOOGLE_IDENTITY)
                    .bind(UserId::new().value().to_string())
                    .bind(identity.sub())
                    .bind(identity.email())
                    .bind(identity.display_name())
                    .fetch_one(&self.pool)
                    .await
                    .map_err(|err| UserRepositoryError::Database(err.to_string()))?;

                user_from_row(row).map_err(|err| UserRepositoryError::Database(err.to_string()))
            }
        }
    };
}

impl_user_repository!(SQLiteUserRepository, SqlitePool);
impl_user_repository!(PostgresUserRepository, PgPool);
