use sha2::{Digest, Sha256};

/// Cookieに入れる推測不能なランダム値。
/// DBには hash() の値だけを保存し、DBが漏れてもセッションを乗っ取れないようにする。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpaqueToken {
    value: String,
}

impl OpaqueToken {
    pub fn generate() -> Self {
        let mut bytes = [0u8; 32];
        getrandom::fill(&mut bytes).expect("OS random number generator is unavailable");

        Self {
            value: hex::encode(bytes),
        }
    }

    pub fn from_value(value: String) -> Self {
        Self { value }
    }

    pub fn value(&self) -> &str {
        &self.value
    }

    pub fn hash(&self) -> String {
        hex::encode(Sha256::digest(self.value.as_bytes()))
    }
}

#[cfg(test)]
mod opaque_token_tests {
    use crate::auth::token::OpaqueToken;

    #[test]
    fn test_generated_tokens_are_unique_and_256_bit() {
        let first = OpaqueToken::generate();
        let second = OpaqueToken::generate();

        assert_ne!(first, second);
        assert_eq!(first.value().len(), 64);
    }

    #[test]
    fn test_hash_is_stable_and_differs_from_value() {
        let token = OpaqueToken::from_value("token".to_string());
        let same_token = OpaqueToken::from_value("token".to_string());

        assert_eq!(token.hash(), same_token.hash());
        assert_ne!(token.hash(), token.value());
    }
}
