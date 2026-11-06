//! `VideoObjectStorage` の Cloudflare R2 実装。署名は rusty-s3、HEAD / DELETE は短命の署名付き URL を reqwest で叩く。
//!
//! reqwest のエラーは URL（署名 `X-Amz-Signature` など）を含むので、文字列にするときは必ず `without_url()` を通す。

use std::time::Duration;

use async_trait::async_trait;
use reqwest::{
    StatusCode, Url,
    header::{CONTENT_LENGTH, HeaderMap},
};
use rusty_s3::{Bucket, Credentials, S3Action, UrlStyle};

use super::{error::StorageError, video_object_storage::VideoObjectStorage};

/// HEAD / DELETE 用の署名の有効期間。すぐ使うので短くてよい。
const CONTROL_URL_TTL: Duration = Duration::from_secs(60);
/// HEAD / DELETE のリクエストのタイムアウト。
const REQUEST_TIMEOUT: Duration = Duration::from_secs(10);

pub struct R2VideoObjectStorage {
    bucket: Bucket,
    credentials: Credentials,
    client: reqwest::Client,
}

impl R2VideoObjectStorage {
    /// エンドポイント `https://{account_id}.r2.cloudflarestorage.com`、リージョン `auto`、パス形式。
    pub fn new(
        account_id: String,
        bucket: String,
        access_key_id: String,
        secret_access_key: String,
    ) -> Self {
        let endpoint: Url = format!("https://{account_id}.r2.cloudflarestorage.com")
            .parse()
            .expect("R2 のエンドポイント URL が不正（R2_ACCOUNT_ID を確認）");
        let bucket = Bucket::new(endpoint, UrlStyle::Path, bucket, "auto")
            .expect("R2 のバケット設定が不正");
        let client = reqwest::Client::builder()
            .timeout(REQUEST_TIMEOUT)
            .build()
            .expect("reqwest クライアントの生成に失敗");
        Self {
            bucket,
            credentials: Credentials::new(access_key_id, secret_access_key),
            client,
        }
    }
}

#[async_trait]
impl VideoObjectStorage for R2VideoObjectStorage {
    fn presign_put(
        &self,
        key: &str,
        content_type: &str,
        content_length: Option<u64>,
        ttl: Duration,
    ) -> String {
        let mut action = self.bucket.put_object(Some(&self.credentials), key);
        action.headers_mut().insert("content-type", content_type);
        if let Some(len) = content_length {
            // 署名に含めると、違うサイズの本文は R2 が拒否する（申告サイズのすり抜け防止）
            action.headers_mut().insert("content-length", len.to_string());
        }
        action.sign(ttl).to_string()
    }

    fn presign_get(&self, key: &str, ttl: Duration, download_filename: Option<&str>) -> String {
        let mut action = self.bucket.get_object(Some(&self.credentials), key);
        if let Some(name) = download_filename {
            action
                .query_mut()
                .insert("response-content-disposition", format!(r#"attachment; filename="{name}""#));
        }
        action.sign(ttl).to_string()
    }

    async fn head(&self, key: &str) -> Result<Option<u64>, StorageError> {
        let url = self
            .bucket
            .head_object(Some(&self.credentials), key)
            .sign(CONTROL_URL_TTL);
        let res = self
            .client
            .head(url)
            .send()
            .await
            .map_err(|e| StorageError::Request(e.without_url().to_string()))?;
        match res.status() {
            // HEAD の応答では `Response::content_length()` が常に 0 になるので、ヘッダーを直接読む
            StatusCode::OK => Ok(Some(content_length_from_headers(res.headers())?)),
            StatusCode::NOT_FOUND => Ok(None),
            status => Err(StorageError::Request(format!("HEAD status {status}"))),
        }
    }

    async fn delete(&self, key: &str) -> Result<(), StorageError> {
        let url = self
            .bucket
            .delete_object(Some(&self.credentials), key)
            .sign(CONTROL_URL_TTL);
        let res = self
            .client
            .delete(url)
            .send()
            .await
            .map_err(|e| StorageError::Request(e.without_url().to_string()))?;
        // S3 互換の DELETE は対象が無くても 204 を返す
        if res.status().is_success() || res.status() == StatusCode::NOT_FOUND {
            Ok(())
        } else {
            Err(StorageError::Request(format!("DELETE status {}", res.status())))
        }
    }
}

/// `Content-Length` ヘッダーの値（バイト）。無い・数値でなければエラー。
fn content_length_from_headers(headers: &HeaderMap) -> Result<u64, StorageError> {
    headers
        .get(CONTENT_LENGTH)
        .ok_or_else(|| StorageError::Request("HEAD の応答に Content-Length が無い".to_string()))?
        .to_str()
        .ok()
        .and_then(|v| v.trim().parse::<u64>().ok())
        .ok_or_else(|| StorageError::Request("HEAD の応答の Content-Length が不正".to_string()))
}

#[cfg(test)]
mod r2_video_object_storage_test {
    use super::*;

    fn r2() -> R2VideoObjectStorage {
        R2VideoObjectStorage::new("acc".into(), "bucket".into(), "AKID".into(), "SECRET".into())
    }

    #[test]
    fn r2_presigned_put_targets_bucket_and_key() {
        let url = r2().presign_put(
            "video-jobs/x/input",
            "video/mp4",
            Some(10_000_000),
            Duration::from_secs(900),
        );
        assert!(url.starts_with("https://acc.r2.cloudflarestorage.com/bucket/video-jobs/x/input?"));
        assert!(url.contains("X-Amz-Expires=900"));
        // Content-Length と Content-Type も署名に含める（違うサイズの本文は R2 が拒否する）
        assert!(url.contains("X-Amz-SignedHeaders=content-length%3Bcontent-type%3Bhost"));
    }

    #[test]
    fn r2_presigned_put_without_length_signs_only_content_type() {
        let url = r2().presign_put("video-jobs/x/output.mp4", "video/mp4", None, Duration::from_secs(1800));
        assert!(url.contains("X-Amz-SignedHeaders=content-type%3Bhost"));
    }

    #[test]
    fn content_length_header_is_parsed() {
        use reqwest::header::{CONTENT_LENGTH, HeaderMap, HeaderValue};
        let mut headers = HeaderMap::new();
        headers.insert(CONTENT_LENGTH, HeaderValue::from_static("209715201"));
        assert_eq!(content_length_from_headers(&headers).unwrap(), 209_715_201);

        assert!(content_length_from_headers(&HeaderMap::new()).is_err());

        let mut garbage = HeaderMap::new();
        garbage.insert(CONTENT_LENGTH, HeaderValue::from_static("abc"));
        assert!(content_length_from_headers(&garbage).is_err());
    }

    #[test]
    fn r2_presigned_get_sets_download_filename() {
        let url = r2().presign_get(
            "video-jobs/x/output.mp4",
            Duration::from_secs(600),
            Some("cutout-12345678.mp4"),
        );
        assert!(url.starts_with("https://acc.r2.cloudflarestorage.com/bucket/video-jobs/x/output.mp4?"));
        assert!(url.contains("X-Amz-Expires=600"));
        let parsed: Url = url.parse().unwrap();
        let disposition = parsed
            .query_pairs()
            .find(|(k, _)| k == "response-content-disposition")
            .map(|(_, v)| v.into_owned());
        assert_eq!(disposition.as_deref(), Some(r#"attachment; filename="cutout-12345678.mp4""#));

        let plain = r2().presign_get("video-jobs/x/input", Duration::from_secs(1800), None);
        assert!(!plain.contains("response-content-disposition"));
    }
}
