use std::{
    sync::Arc,
    time::{SystemTime, UNIX_EPOCH},
};

use crate::{
    auth::{
        error::{AuthUseCaseError, LoginRequiredError},
        google::{client::GoogleAuthClient, error::GoogleLoginConfigError},
        login_session::repository::repository::LoginSessionRepository,
        token::OpaqueToken,
    },
    config::LOGIN_SESSION_LIFETIME,
    user::{repository::repository::UserRepository, user::User},
};

pub struct LoginStart {
    pub authorization_url: String,
    pub state: OpaqueToken,
}

pub struct AuthUseCase {
    // Googleの認証情報が未設定でも、匿名の画像編集はそのまま動かせるようOptionで持つ
    google_client: Option<Arc<dyn GoogleAuthClient>>,
    user_repository: Arc<dyn UserRepository>,
    login_session_repository: Arc<dyn LoginSessionRepository>,
}

impl AuthUseCase {
    pub fn new(
        google_client: Option<Arc<dyn GoogleAuthClient>>,
        user_repository: Arc<dyn UserRepository>,
        login_session_repository: Arc<dyn LoginSessionRepository>,
    ) -> Self {
        Self {
            google_client,
            user_repository,
            login_session_repository,
        }
    }

    pub fn start_login(&self) -> Result<LoginStart, AuthUseCaseError> {
        let state = OpaqueToken::generate();
        let authorization_url = self.google_client()?.authorization_url(state.value());

        Ok(LoginStart {
            authorization_url,
            state,
        })
    }

    pub async fn complete_login(&self, code: &str) -> Result<OpaqueToken, AuthUseCaseError> {
        let identity = self.google_client()?.fetch_identity(code).await?;
        let user = self
            .user_repository
            .upsert_by_google_identity(&identity)
            .await?;

        let now = unix_now();
        self.login_session_repository.delete_expired(now).await?;

        let token = OpaqueToken::generate();
        self.login_session_repository
            .create(
                &token.hash(),
                user.id(),
                now + LOGIN_SESSION_LIFETIME.as_secs() as i64,
            )
            .await?;

        Ok(token)
    }

    pub async fn current_user(
        &self,
        token: &OpaqueToken,
    ) -> Result<Option<User>, AuthUseCaseError> {
        Ok(self
            .login_session_repository
            .find_user(&token.hash(), unix_now())
            .await?)
    }

    pub async fn logout(&self, token: &OpaqueToken) -> Result<(), AuthUseCaseError> {
        Ok(self.login_session_repository.delete(&token.hash()).await?)
    }

    /// 退会。ユーザーを消すと、他の端末の分も含めてログインセッションが連鎖削除される。
    pub async fn delete_account(
        &self,
        token: Option<&OpaqueToken>,
    ) -> Result<(), AuthUseCaseError> {
        let token = token.ok_or(LoginRequiredError::LoginRequired)?;
        let user = self
            .current_user(token)
            .await?
            .ok_or(LoginRequiredError::LoginRequired)?;

        Ok(self.user_repository.delete(user.id()).await?)
    }

    fn google_client(&self) -> Result<&Arc<dyn GoogleAuthClient>, AuthUseCaseError> {
        self.google_client
            .as_ref()
            .ok_or(GoogleLoginConfigError::NotConfigured.into())
    }
}

fn unix_now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("System clock is before the UNIX epoch")
        .as_secs() as i64
}

#[cfg(test)]
mod auth_usecase_tests {
    use std::sync::Arc;

    use async_trait::async_trait;
    use sqlx::sqlite::SqlitePoolOptions;

    use crate::{
        auth::{
            error::{AuthUseCaseError, LoginRequiredError},
            google::{
                client::GoogleAuthClient,
                error::{GoogleAuthError, GoogleLoginConfigError},
            },
            login_session::repository::repository_impl::SQLiteLoginSessionRepository,
            token::OpaqueToken,
            usecase::AuthUseCase,
        },
        user::{
            google_identity::GoogleIdentity, repository::repository_impl::SQLiteUserRepository,
        },
    };

    struct FakeGoogleClient {
        verified: bool,
    }

    #[async_trait]
    impl GoogleAuthClient for FakeGoogleClient {
        fn authorization_url(&self, state: &str) -> String {
            format!("https://google.test/auth?state={state}")
        }

        async fn fetch_identity(&self, code: &str) -> Result<GoogleIdentity, GoogleAuthError> {
            if !self.verified {
                return Err(GoogleAuthError::EmailNotVerified(code.to_string()));
            }
            Ok(GoogleIdentity::new(
                "sub-1".to_string(),
                format!("{code}@example.com"),
                Some("Test User".to_string()),
            ))
        }
    }

    async fn usecase(google_client: Option<Arc<dyn GoogleAuthClient>>) -> AuthUseCase {
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap();
        sqlx::migrate!("./migrations").run(&pool).await.unwrap();

        AuthUseCase::new(
            google_client,
            Arc::new(SQLiteUserRepository::new(pool.clone())),
            Arc::new(SQLiteLoginSessionRepository::new(pool)),
        )
    }

    #[tokio::test]
    async fn test_login_then_logout() {
        let usecase = usecase(Some(Arc::new(FakeGoogleClient { verified: true }))).await;

        let token = usecase.complete_login("first").await.unwrap();
        let user = usecase.current_user(&token).await.unwrap().unwrap();
        assert_eq!(user.email(), "first@example.com");
        assert_eq!(user.display_name(), Some("Test User"));

        usecase.logout(&token).await.unwrap();
        assert_eq!(usecase.current_user(&token).await.unwrap(), None);
    }

    #[tokio::test]
    async fn test_same_google_account_is_same_user() {
        let usecase = usecase(Some(Arc::new(FakeGoogleClient { verified: true }))).await;

        let first = usecase.complete_login("old").await.unwrap();
        let second = usecase.complete_login("new").await.unwrap();

        let first_user = usecase.current_user(&first).await.unwrap().unwrap();
        let second_user = usecase.current_user(&second).await.unwrap().unwrap();
        assert_eq!(first_user.id(), second_user.id());
        assert_eq!(second_user.email(), "new@example.com");
    }

    #[tokio::test]
    async fn test_delete_account_ends_every_session() {
        let usecase = usecase(Some(Arc::new(FakeGoogleClient { verified: true }))).await;
        let this_device = usecase.complete_login("first").await.unwrap();
        let other_device = usecase.complete_login("first").await.unwrap();
        let deleted_user = usecase.current_user(&this_device).await.unwrap().unwrap();

        usecase.delete_account(Some(&this_device)).await.unwrap();

        assert_eq!(usecase.current_user(&this_device).await.unwrap(), None);
        assert_eq!(usecase.current_user(&other_device).await.unwrap(), None);

        // 同じGoogleアカウントで再度ログインすると、別のユーザーとして作り直される
        let token = usecase.complete_login("first").await.unwrap();
        let new_user = usecase.current_user(&token).await.unwrap().unwrap();
        assert_ne!(new_user.id(), deleted_user.id());
    }

    #[tokio::test]
    async fn test_delete_account_requires_valid_session() {
        let usecase = usecase(Some(Arc::new(FakeGoogleClient { verified: true }))).await;

        for token in [None, Some(&OpaqueToken::generate())] {
            assert!(matches!(
                usecase.delete_account(token).await,
                Err(AuthUseCaseError::LoginRequired(
                    LoginRequiredError::LoginRequired
                ))
            ));
        }
    }

    #[tokio::test]
    async fn test_unknown_token_has_no_user() {
        let usecase = usecase(Some(Arc::new(FakeGoogleClient { verified: true }))).await;

        let user = usecase
            .current_user(&OpaqueToken::generate())
            .await
            .unwrap();

        assert_eq!(user, None);
    }

    #[tokio::test]
    async fn test_rejected_identity_creates_no_session() {
        let usecase = usecase(Some(Arc::new(FakeGoogleClient { verified: false }))).await;

        let result = usecase.complete_login("code").await;

        assert!(matches!(
            result,
            Err(AuthUseCaseError::GoogleAuth(
                GoogleAuthError::EmailNotVerified(_)
            ))
        ));
    }

    #[tokio::test]
    async fn test_login_fails_without_google_config() {
        let usecase = usecase(None).await;

        assert!(matches!(
            usecase.start_login(),
            Err(AuthUseCaseError::GoogleLoginConfig(
                GoogleLoginConfigError::NotConfigured
            ))
        ));
    }
}
