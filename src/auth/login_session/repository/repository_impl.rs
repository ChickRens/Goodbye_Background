use async_trait::async_trait;
use sqlx::{PgPool, SqlitePool};

use crate::{
    auth::login_session::repository::{
        error::LoginSessionRepositoryError, repository::LoginSessionRepository,
    },
    user::{
        id::UserId,
        repository::repository_impl::{UserRow, user_from_row},
        user::User,
    },
};

fn database_error(err: impl ToString) -> LoginSessionRepositoryError {
    LoginSessionRepositoryError::Database(err.to_string())
}

// SQLはSQLite/PostgreSQL共通なので、型の違うpoolごとの実装はマクロで生成する。
macro_rules! impl_login_session_repository {
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
        impl LoginSessionRepository for $name {
            async fn create(
                &self,
                token_hash: &str,
                user_id: &UserId,
                expires_at: i64,
            ) -> Result<(), LoginSessionRepositoryError> {
                sqlx::query(
                    "INSERT INTO login_sessions (token_hash, user_id, expires_at) VALUES ($1, $2, $3)",
                )
                .bind(token_hash)
                .bind(user_id.value().to_string())
                .bind(expires_at)
                .execute(&self.pool)
                .await
                .map_err(database_error)?;

                Ok(())
            }

            async fn find_user(
                &self,
                token_hash: &str,
                now: i64,
            ) -> Result<Option<User>, LoginSessionRepositoryError> {
                let row: Option<UserRow> = sqlx::query_as(
                    "SELECT users.id, users.email, users.display_name
                     FROM login_sessions
                     JOIN users ON users.id = login_sessions.user_id
                     WHERE login_sessions.token_hash = $1 AND login_sessions.expires_at > $2",
                )
                .bind(token_hash)
                .bind(now)
                .fetch_optional(&self.pool)
                .await
                .map_err(database_error)?;

                row.map(user_from_row).transpose().map_err(database_error)
            }

            async fn delete(&self, token_hash: &str) -> Result<(), LoginSessionRepositoryError> {
                sqlx::query("DELETE FROM login_sessions WHERE token_hash = $1")
                    .bind(token_hash)
                    .execute(&self.pool)
                    .await
                    .map_err(database_error)?;

                Ok(())
            }

            async fn delete_expired(&self, now: i64) -> Result<(), LoginSessionRepositoryError> {
                sqlx::query("DELETE FROM login_sessions WHERE expires_at <= $1")
                    .bind(now)
                    .execute(&self.pool)
                    .await
                    .map_err(database_error)?;

                Ok(())
            }
        }
    };
}

impl_login_session_repository!(SQLiteLoginSessionRepository, SqlitePool);
impl_login_session_repository!(PostgresLoginSessionRepository, PgPool);

#[cfg(test)]
mod login_session_repository_tests {
    use sqlx::{postgres::PgPoolOptions, sqlite::SqlitePoolOptions};

    use crate::{
        auth::login_session::repository::{
            repository::LoginSessionRepository,
            repository_impl::{PostgresLoginSessionRepository, SQLiteLoginSessionRepository},
        },
        user::{
            google_identity::GoogleIdentity,
            repository::{
                repository::UserRepository,
                repository_impl::{PostgresUserRepository, SQLiteUserRepository},
            },
        },
    };

    async fn assert_session_lifecycle(
        users: &dyn UserRepository,
        sessions: &dyn LoginSessionRepository,
        sub: &str,
    ) {
        let identity = GoogleIdentity::new(sub.to_string(), "a@example.com".to_string(), None);
        let user = users.upsert_by_google_identity(&identity).await.unwrap();

        let renamed = GoogleIdentity::new(
            sub.to_string(),
            "b@example.com".to_string(),
            Some("B".to_string()),
        );
        let updated = users.upsert_by_google_identity(&renamed).await.unwrap();
        assert_eq!(user.id(), updated.id());

        let hash = format!("{sub}-hash");
        sessions.create(&hash, user.id(), 100).await.unwrap();

        // 有効期限の境界: expires_at ちょうどで無効になる
        let found = sessions.find_user(&hash, 99).await.unwrap().unwrap();
        assert_eq!(found, updated);
        assert_eq!(sessions.find_user(&hash, 100).await.unwrap(), None);

        sessions.delete_expired(99).await.unwrap();
        assert!(sessions.find_user(&hash, 99).await.unwrap().is_some());

        sessions.delete_expired(100).await.unwrap();
        assert_eq!(sessions.find_user(&hash, 99).await.unwrap(), None);
    }

    /// 退会したユーザーのセッションだけが消えることを確かめ、消えたはずのセッションのhashを返す。
    /// find_user はusersとJOINするので、行が残っていてもNoneになる。連鎖削除の確認は呼び出し側で行数を見る。
    async fn delete_one_of_two_users(
        users: &dyn UserRepository,
        sessions: &dyn LoginSessionRepository,
        sub: &str,
    ) -> String {
        let deleted = GoogleIdentity::new(format!("{sub}-a"), "a@example.com".to_string(), None);
        let kept = GoogleIdentity::new(format!("{sub}-b"), "b@example.com".to_string(), None);
        let deleted = users.upsert_by_google_identity(&deleted).await.unwrap();
        let kept = users.upsert_by_google_identity(&kept).await.unwrap();

        let deleted_hash = format!("{sub}-a-hash");
        let kept_hash = format!("{sub}-b-hash");
        sessions
            .create(&deleted_hash, deleted.id(), 100)
            .await
            .unwrap();
        sessions.create(&kept_hash, kept.id(), 100).await.unwrap();

        users.delete(deleted.id()).await.unwrap();

        assert_eq!(sessions.find_user(&deleted_hash, 99).await.unwrap(), None);
        assert_eq!(
            sessions.find_user(&kept_hash, 99).await.unwrap(),
            Some(kept)
        );

        deleted_hash
    }

    const COUNT_SESSIONS: &str = "SELECT COUNT(*) FROM login_sessions WHERE token_hash = $1";

    #[tokio::test]
    async fn test_sqlite_session_lifecycle() {
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap();
        sqlx::migrate!("./migrations").run(&pool).await.unwrap();

        assert_session_lifecycle(
            &SQLiteUserRepository::new(pool.clone()),
            &SQLiteLoginSessionRepository::new(pool.clone()),
            "sub",
        )
        .await;

        let deleted_hash = delete_one_of_two_users(
            &SQLiteUserRepository::new(pool.clone()),
            &SQLiteLoginSessionRepository::new(pool.clone()),
            "delete",
        )
        .await;
        let remaining: i64 = sqlx::query_scalar(COUNT_SESSIONS)
            .bind(deleted_hash)
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(remaining, 0);
    }

    // 本物のPostgreSQLが必要なので、次のように明示したときだけ動かす:
    // TEST_DATABASE_URL_POSTGRES=postgres://... cargo test -- --ignored postgres
    #[tokio::test]
    #[ignore]
    async fn test_postgres_session_lifecycle() {
        let url = std::env::var("TEST_DATABASE_URL_POSTGRES").unwrap();
        let pool = PgPoolOptions::new()
            .max_connections(1)
            .connect(&url)
            .await
            .unwrap();
        sqlx::migrate!("./migrations").run(&pool).await.unwrap();

        assert_session_lifecycle(
            &PostgresUserRepository::new(pool.clone()),
            &PostgresLoginSessionRepository::new(pool.clone()),
            &uuid::Uuid::new_v4().to_string(),
        )
        .await;

        let deleted_hash = delete_one_of_two_users(
            &PostgresUserRepository::new(pool.clone()),
            &PostgresLoginSessionRepository::new(pool.clone()),
            &uuid::Uuid::new_v4().to_string(),
        )
        .await;
        let remaining: i64 = sqlx::query_scalar(COUNT_SESSIONS)
            .bind(deleted_hash)
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(remaining, 0);
    }
}
