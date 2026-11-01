use crate::user::id::UserId;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct User {
    id: UserId,
    email: String,
    display_name: Option<String>,
}

impl User {
    pub fn new(id: UserId, email: String, display_name: Option<String>) -> Self {
        Self {
            id,
            email,
            display_name,
        }
    }

    pub fn id(&self) -> &UserId {
        &self.id
    }

    pub fn email(&self) -> &str {
        &self.email
    }

    pub fn display_name(&self) -> Option<&str> {
        self.display_name.as_deref()
    }
}
