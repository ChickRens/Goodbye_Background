//! メンテナンス系: TTL 付きインメモリストアと、期限切れエントリの定期削除。

pub mod deleting_expired_entries;
pub mod ttl_store;
