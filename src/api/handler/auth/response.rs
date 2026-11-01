use crate::user::user::User;

#[derive(serde::Serialize)]
pub struct CurrentUserResponse {
    user: Option<UserResponse>,
}

#[derive(serde::Serialize)]
struct UserResponse {
    email: String,
    display_name: Option<String>,
}

impl CurrentUserResponse {
    pub fn new(user: Option<User>) -> Self {
        Self {
            user: user.map(|user| UserResponse {
                email: user.email().to_string(),
                display_name: user.display_name().map(str::to_string),
            }),
        }
    }
}
