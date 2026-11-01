use crate::{
    auth::{
        google::error::{GoogleAuthError, GoogleLoginConfigError},
        login_session::repository::error::LoginSessionRepositoryError,
    },
    parent_error,
    user::repository::error::UserRepositoryError,
};

parent_error!(
    pub enum AuthUseCaseError {
        GoogleLoginConfig(GoogleLoginConfigError),
        GoogleAuth(GoogleAuthError),
        UserRepository(UserRepositoryError),
        LoginSessionRepository(LoginSessionRepositoryError),
    }
);
