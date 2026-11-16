//! SQLite 版の DailyQuota。
//!
//! 並行性: `try_consume` は 1 トランザクションで読み書きする。main.rs の SQLite プールは
//! `max_connections(1)` なので、トランザクション同士は接続の取り合いで直列化される。

use async_trait::async_trait;
use sqlx::SqlitePool;

use super::daily_quota::{DAILY_LIMIT, DailyQuota, QuotaKeys, remaining_from};
use super::error::QuotaError;

/// この日数より古い行は消費時に削除する。
const RETENTION_DAYS: i64 = 7;

pub struct SqliteDailyQuota {
    pool: SqlitePool,
}

impl SqliteDailyQuota {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }
}

fn db_err(e: sqlx::Error) -> QuotaError {
    QuotaError::Database(e.to_string())
}

async fn count_of(
    conn: &mut sqlx::SqliteConnection,
    day: i64,
    kind: &str,
    key: &str,
) -> Result<i64, QuotaError> {
    let c: Option<i64> = sqlx::query_scalar(
        "SELECT count FROM video_quota_usage WHERE day = ? AND key_kind = ? AND key = ?",
    )
    .bind(day)
    .bind(kind)
    .bind(key)
    .fetch_optional(conn)
    .await
    .map_err(db_err)?;
    Ok(c.unwrap_or(0))
}

#[async_trait]
impl DailyQuota for SqliteDailyQuota {
    async fn remaining(&self, keys: &QuotaKeys, day: i64) -> Result<u8, QuotaError> {
        let mut conn = self.pool.acquire().await.map_err(db_err)?;
        let [(k1, v1), (k2, v2)] = keys.rows();
        let a = count_of(&mut conn, day, k1, &v1).await?;
        let b = count_of(&mut conn, day, k2, &v2).await?;
        Ok(remaining_from(a, b))
    }

    async fn try_consume(&self, keys: &QuotaKeys, day: i64) -> Result<u8, QuotaError> {
        let mut tx = self.pool.begin().await.map_err(db_err)?;
        let rows = keys.rows();
        let mut counts = [0i64; 2];
        for (i, (kind, key)) in rows.iter().enumerate() {
            counts[i] = count_of(&mut tx, day, kind, key).await?;
        }
        if counts.iter().any(|&c| c >= DAILY_LIMIT) {
            return Err(QuotaError::Exceeded(String::new()));
        }
        for (kind, key) in &rows {
            sqlx::query(
                "INSERT INTO video_quota_usage (day, key_kind, key, count) VALUES (?, ?, ?, 1) \
                 ON CONFLICT (day, key_kind, key) DO UPDATE SET count = count + 1",
            )
            .bind(day)
            .bind(kind)
            .bind(key)
            .execute(&mut *tx)
            .await
            .map_err(db_err)?;
        }
        sqlx::query("DELETE FROM video_quota_usage WHERE day < ?")
            .bind(day - RETENTION_DAYS)
            .execute(&mut *tx)
            .await
            .map_err(db_err)?;
        tx.commit().await.map_err(db_err)?;
        Ok(remaining_from(counts[0] + 1, counts[1] + 1))
    }

    async fn refund(&self, keys: &QuotaKeys, day: i64) -> Result<(), QuotaError> {
        let mut tx = self.pool.begin().await.map_err(db_err)?;
        for (kind, key) in &keys.rows() {
            sqlx::query(
                "UPDATE video_quota_usage SET count = MAX(count - 1, 0) \
                 WHERE day = ? AND key_kind = ? AND key = ?",
            )
            .bind(day)
            .bind(kind)
            .bind(key)
            .execute(&mut *tx)
            .await
            .map_err(db_err)?;
        }
        tx.commit().await.map_err(db_err)?;
        Ok(())
    }
}

#[cfg(test)]
mod daily_quota_sqlite_test {
    use std::net::IpAddr;

    use sqlx::sqlite::SqlitePoolOptions;
    use uuid::Uuid;

    use super::*;

    async fn quota() -> SqliteDailyQuota {
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap();
        sqlx::migrate!("./migrations/sqlite").run(&pool).await.unwrap();
        SqliteDailyQuota::new(pool)
    }

    fn client(n: u128) -> Uuid {
        Uuid::from_u128(n + 1)
    }

    fn keys_with(client_id: Uuid, ip: &str) -> QuotaKeys {
        QuotaKeys { client_id, ip: ip.parse::<IpAddr>().unwrap() }
    }

    fn keys(ip: &str) -> QuotaKeys {
        keys_with(client(0), ip)
    }

    fn is_exceeded(r: Result<u8, QuotaError>) -> bool {
        matches!(r, Err(QuotaError::Exceeded(_)))
    }

    #[tokio::test]
    async fn three_per_day_then_exceeded() {
        let q = quota().await;
        let k = keys("10.0.0.1");
        assert_eq!(q.try_consume(&k, 100).await, Ok(2));
        q.try_consume(&k, 100).await.unwrap();
        assert_eq!(q.try_consume(&k, 100).await, Ok(0));
        assert!(is_exceeded(q.try_consume(&k, 100).await));
        assert_eq!(q.try_consume(&k, 101).await, Ok(2));
    }

    #[tokio::test]
    async fn either_key_blocks() {
        let q = quota().await;
        for i in 0..3 {
            q.try_consume(&keys_with(client(i), "10.0.0.1"), 100).await.unwrap();
        }
        assert!(is_exceeded(q.try_consume(&keys_with(client(9), "10.0.0.1"), 100).await));
        assert_eq!(q.remaining(&keys_with(client(9), "10.0.0.2"), 100).await, Ok(3));
        assert_eq!(q.remaining(&keys_with(client(9), "10.0.0.1"), 100).await, Ok(0));
    }

    #[tokio::test]
    async fn refund_restores_and_never_negative() {
        let q = quota().await;
        let k = keys("10.0.0.1");
        q.try_consume(&k, 100).await.unwrap();
        q.refund(&k, 100).await.unwrap();
        q.refund(&k, 100).await.unwrap();
        assert_eq!(q.remaining(&k, 100).await, Ok(3));
    }

    #[tokio::test]
    async fn old_rows_are_deleted_on_consume() {
        let q = quota().await;
        let k = keys("10.0.0.1");
        q.try_consume(&k, 100).await.unwrap();
        q.try_consume(&k, 108).await.unwrap();
        let n: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM video_quota_usage WHERE day = 100")
            .fetch_one(&q.pool)
            .await
            .unwrap();
        assert_eq!(n, 0);
    }
}
