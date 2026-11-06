//! ワーカーからの通知の HMAC-SHA256 署名（仕様 §4.6）。
//!
//! ヘッダー `X-Signature-Timestamp`（UNIX 秒）と `X-Signature`（`hex(HMAC-SHA256(secret, "{ts}." + 本文))`）。
//! 時刻のずれが `MAX_SKEW_S` を超えたもの・形式が不正なものは `Invalid`。比較は定数時間で行う。

use hmac::{Hmac, KeyInit, Mac};
use sha2::Sha256;

use crate::{common::traits::ErrorType, leaf_error};

leaf_error!(
    pub enum SignatureError {
        Invalid => ("INVALID_SIGNATURE", ErrorType::Unauthorized),
    }
);

/// 許容する時刻のずれ（秒）。
pub const MAX_SKEW_S: u64 = 300;

type HmacSha256 = Hmac<Sha256>;

fn mac_for(secret: &[u8], timestamp: u64, body: &[u8]) -> HmacSha256 {
    // HMAC はどんな長さの鍵も受け付けるので失敗しない
    let mut mac = HmacSha256::new_from_slice(secret).expect("HMAC は任意長の鍵を受け付ける");
    mac.update(timestamp.to_string().as_bytes());
    mac.update(b".");
    mac.update(body);
    mac
}

/// 署名（小文字の16進）を作る。
pub fn sign(secret: &[u8], timestamp: u64, body: &[u8]) -> String {
    hex::encode(mac_for(secret, timestamp, body).finalize().into_bytes())
}

/// 署名を検証する。`now_s` は現在の UNIX 秒。
pub fn verify(
    secret: &[u8],
    timestamp_header: &str,
    signature_header: &str,
    body: &[u8],
    now_s: u64,
) -> Result<(), SignatureError> {
    let timestamp: u64 = timestamp_header
        .trim()
        .parse()
        .map_err(|_| SignatureError::Invalid)?;
    if timestamp.abs_diff(now_s) > MAX_SKEW_S {
        return Err(SignatureError::Invalid);
    }
    let expected = hex::decode(signature_header.trim()).map_err(|_| SignatureError::Invalid)?;
    mac_for(secret, timestamp, body)
        .verify_slice(&expected)
        .map_err(|_| SignatureError::Invalid)
}

#[cfg(test)]
mod signature_test {
    use super::*;

    fn hmac_sha256(key: &[u8], msg: &[u8]) -> Vec<u8> {
        let mut m = HmacSha256::new_from_slice(key).unwrap();
        m.update(msg);
        m.finalize().into_bytes().to_vec()
    }

    #[test]
    fn sign_known_vector() {
        assert_eq!(
            sign(b"secret", 1_700_000_000, br#"{"a":1}"#),
            hex::encode(hmac_sha256(b"secret", br#"1700000000.{"a":1}"#))
        );
    }

    #[test]
    fn verify_rejects_tamper_and_skew() {
        let s = sign(b"k", 1000, b"body");
        assert!(verify(b"k", "1000", &s, b"body", 1000).is_ok());
        assert!(verify(b"k", "1000", &s, b"bodx", 1000).is_err());
        assert!(verify(b"k", "1000", &s, b"body", 1301).is_err());
        assert!(verify(b"k", "1000", &s, b"body", 699).is_err());
    }

    #[test]
    fn verify_accepts_skew_boundary_and_rejects_bad_format() {
        let s = sign(b"k", 1000, b"body");
        assert!(verify(b"k", "1000", &s, b"body", 1300).is_ok());
        assert!(verify(b"k", "1000", &s, b"body", 700).is_ok());
        assert_eq!(verify(b"k", "abc", &s, b"body", 1000), Err(SignatureError::Invalid));
        assert_eq!(verify(b"k", "1000", "zz", b"body", 1000), Err(SignatureError::Invalid));
        assert_eq!(verify(b"x", "1000", &s, b"body", 1000), Err(SignatureError::Invalid));
        // 大文字の16進も同じバイト列として受け付ける
        assert!(verify(b"k", "1000", &s.to_uppercase(), b"body", 1000).is_ok());
    }
}
