/// Googleが確認済みの本人情報。ユーザーの同一性は変更され得るemailではなくsubで判定する。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GoogleIdentity {
    sub: String,
    email: String,
    display_name: Option<String>,
}

impl GoogleIdentity {
    pub fn new(sub: String, email: String, display_name: Option<String>) -> Self {
        Self {
            sub,
            email,
            display_name,
        }
    }

    pub fn sub(&self) -> &str {
        &self.sub
    }

    pub fn email(&self) -> &str {
        &self.email
    }

    pub fn display_name(&self) -> Option<&str> {
        self.display_name.as_deref()
    }
}
