//! `VideoObjectStorage` の Cloudflare R2 実装。署名は rusty-s3、HEAD / DELETE は短命の署名付き URL を reqwest で叩く。

use std::time::Duration;

use async_trait::async_trait;
use reqwest::{StatusCode, Url};
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
    fn presign_put(&self, key: &str, content_type: &str, ttl: Duration) -> String {
        let mut action = self.bucket.put_object(Some(&self.credentials), key);
        action.headers_mut().insert("content-type", content_type);
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
            .map_err(|e| StorageError::Request(e.to_string()))?;
        match res.status() {
            StatusCode::OK => Ok(Some(res.content_length().ok_or_else(|| {
                StorageError::Request("HEAD の応答に Content-Length が無い".to_string())
            })?)),
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
            .map_err(|e| StorageError::Request(e.to_string()))?;
        // S3 互換の DELETE は対象が無くても 204 を返す
        if res.status().is_success() || res.status() == StatusCode::NOT_FOUND {
            Ok(())
        } else {
            Err(StorageError::Request(format!("DELETE status {}", res.status())))
        }
    }
}

#[cfg(test)]
mod r2_video_object_storage_test {
    use super::*;

    fn r2() -> R2VideoObjectStorage {
        R2VideoObjectStorage::new("acc".into(), "bucket".into(), "AKID".into(), "SECRET".into())
    }

    #[test]
    fn r2_presigned_put_targets_bucket_and_key() {
        let url = r2().presign_put("video-jobs/x/input", "video/mp4", Duration::from_secs(900));
        assert!(url.starts_with("https://acc.r2.cloudflarestorage.com/bucket/video-jobs/x/input?"));
        assert!(url.contains("X-Amz-Expires=900"));
        // Content-Type も署名に含める
        assert!(url.contains("X-Amz-SignedHeaders=content-type%3Bhost"));
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
