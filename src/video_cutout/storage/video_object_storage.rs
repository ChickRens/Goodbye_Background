//! 動画ファイルのオブジェクトストレージ（trait）。
//!
//! ネットワーク待ちだけなので非同期の trait にし、`spawn_blocking` には載せない（仕様 §4.4）。

use std::time::Duration;

use async_trait::async_trait;

use super::error::StorageError;

#[async_trait]
pub trait VideoObjectStorage: Send + Sync {
    /// ブラウザが直接アップロードするための署名付き PUT URL。`content_type` も署名に含める。
    fn presign_put(&self, key: &str, content_type: &str, ttl: Duration) -> String;
    /// 署名付き GET URL。`download_filename` があれば添付ファイルとしてその名前でダウンロードさせる。
    fn presign_get(&self, key: &str, ttl: Duration, download_filename: Option<&str>) -> String;
    /// オブジェクトのサイズ（バイト）。無ければ `None`。
    async fn head(&self, key: &str) -> Result<Option<u64>, StorageError>;
    /// オブジェクトを削除する（無くても成功）。
    async fn delete(&self, key: &str) -> Result<(), StorageError>;
}
