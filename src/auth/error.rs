use crate::{
    auth::{
        google::error::{GoogleAuthError, GoogleLoginConfigError},
        login_session::repository::error::LoginSessionRepositoryError,
    },
    common::traits::ErrorType::Unauthorized,
    leaf_error, parent_error,
    user::repository::error::UserRepositoryError,
};

leaf_error!(
    pub enum LoginRequiredError {
        LoginRequired => ("LOGIN_REQUIRED", Unauthorized),
    }
);

parent_error!(
    pub enum AuthUseCaseError {
        LoginRequired(LoginRequiredError),
        GoogleLoginConfig(GoogleLoginConfigError),
        GoogleAuth(GoogleAuthError),
        UserRepository(UserRepositoryError),
        LoginSessionRepository(LoginSessionRepositoryError),
    }
);
