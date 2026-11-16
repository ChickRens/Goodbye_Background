//! PostgreSQL 版の DailyQuota（SQLite 版と同じ処理。方言だけ違う）。
//!
//! 並行性: 先に 2 行を `ON CONFLICT DO NOTHING` で作り、キー順に `SELECT ... FOR UPDATE` で
//! 行ロックを取ってから判定・加算する。同じ行を取り合う consume は直列化されるので、
//! count=2 から 2 件同時に通ることはない。ロック順を固定してデッドロックも避ける。

use async_trait::async_trait;
use sqlx::PgPool;

use super::daily_quota::{DAILY_LIMIT, DailyQuota, QuotaKeys, remaining_from};
use super::error::QuotaError;

/// この日数より古い行は消費時に削除する。
const RETENTION_DAYS: i64 = 7;

pub struct PostgresDailyQuota {
    pool: PgPool,
}

impl PostgresDailyQuota {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

fn db_err(e: sqlx::Error) -> QuotaError {
    QuotaError::Database(e.to_string())
}

async fn count_of(
    conn: &mut sqlx::PgConnection,
    day: i64,
    kind: &str,
    key: &str,
) -> Result<i64, QuotaError> {
    let c: Option<i64> = sqlx::query_scalar(
        "SELECT count FROM video_quota_usage WHERE day = $1 AND key_kind = $2 AND key = $3",
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
impl DailyQuota for PostgresDailyQuota {
    async fn remaining(&self, keys: &QuotaKeys, day: i64) -> Result<u8, QuotaError> {
        let mut conn = self.pool.acquire().await.map_err(db_err)?;
        let [(k1, v1), (k2, v2)] = keys.rows();
        let a = count_of(&mut conn, day, k1, &v1).await?;
        let b = count_of(&mut conn, day, k2, &v2).await?;
        Ok(remaining_from(a, b))
    }

    async fn try_consume(&self, keys: &QuotaKeys, day: i64) -> Result<u8, QuotaError> {
        let mut tx = self.pool.begin().await.map_err(db_err)?;
        let rows = keys.rows(); // key_kind 順: client, ip（常に同じ順でロックする）
        let mut counts = [0i64; 2];
        for (i, (kind, key)) in rows.iter().enumerate() {
            sqlx::query(
                "INSERT INTO video_quota_usage (day, key_kind, key, count) VALUES ($1, $2, $3, 0) \
                 ON CONFLICT (day, key_kind, key) DO NOTHING",
            )
            .bind(day)
            .bind(kind)
            .bind(key)
            .execute(&mut *tx)
            .await
            .map_err(db_err)?;
            counts[i] = sqlx::query_scalar(
                "SELECT count FROM video_quota_usage \
                 WHERE day = $1 AND key_kind = $2 AND key = $3 FOR UPDATE",
            )
            .bind(day)
            .bind(kind)
            .bind(key)
            .fetch_one(&mut *tx)
            .await
            .map_err(db_err)?;
        }
        if counts.iter().any(|&c| c >= DAILY_LIMIT) {
            // 0 件の行を作っただけなので、そのまま commit して構わない（値は変わらない）
            tx.commit().await.map_err(db_err)?;
            return Err(QuotaError::Exceeded(String::new()));
        }
        for (kind, key) in &rows {
            sqlx::query(
                "UPDATE video_quota_usage SET count = count + 1 \
                 WHERE day = $1 AND key_kind = $2 AND key = $3",
            )
            .bind(day)
            .bind(kind)
            .bind(key)
            .execute(&mut *tx)
            .await
            .map_err(db_err)?;
        }
        sqlx::query("DELETE FROM video_quota_usage WHERE day < $1")
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
                "UPDATE video_quota_usage SET count = GREATEST(count - 1, 0) \
                 WHERE day = $1 AND key_kind = $2 AND key = $3",
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
