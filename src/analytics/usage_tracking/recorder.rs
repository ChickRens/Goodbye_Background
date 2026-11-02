//! `UsageRecorder` の実装（SQLite 版 / PostgreSQL 版）。
//!
//! 仕組み: `new()` で容量 1024 の mpsc チャネルと、受信して INSERT し続ける tokio タスクを作る。
//! `record()` は `try_send` するだけなので、DB が遅くても API レスポンスは遅れない。
//! その代わりチャネルが満杯だとログは捨てられる（eprintln のみ）。
//!
//! 注意:
//! - `new()` 内で `tokio::spawn` するため、tokio ランタイム内で呼ぶ必要がある
//! - SQLite 版と Postgres 版はプレースホルダ（`?` / `$1`）以外ほぼ同じコードの重複
//! - テーブル定義は `migrations/sqlite/` と `migrations/postgres/` に分かれている（`main.rs` で起動時に適用）

use sqlx::{PgPool, SqlitePool};
use tokio::sync::mpsc::{self, Sender};

use crate::analytics::usage_tracking::{
    event::UsageEvent, operation::UsageOperation, status::UsageStatus,
    usage_recorder::UsageRecorder,
};

pub struct SQLiteUsageRecorder {
    sender: Sender<UsageEvent>,
}

impl UsageRecorder for SQLiteUsageRecorder {
    fn record(&self, event: UsageEvent) {
        if let Err(e) = self.sender.try_send(event) {
            eprintln!("{:?}", e);
        };
    }
}

impl SQLiteUsageRecorder {
    pub fn new(pool: SqlitePool) -> Self {
        let (tx, mut rx) = mpsc::channel::<UsageEvent>(1024);

        tokio::spawn(async move {
            while let Some(event) = rx.recv().await {
                let id = event.id().map(|id| id.value().to_string());

                let operation_str = match event.operation() {
                    UsageOperation::Upload => "upload".to_string(),
                    UsageOperation::Segment => "segment".to_string(),
                    UsageOperation::Save => "save".to_string(),
                    UsageOperation::Redo => "redo".to_string(),
                    UsageOperation::Undo => "undo".to_string(),
                    UsageOperation::GetPreview => "get-preview".to_string(),
                    UsageOperation::GetCompleted => "get-completed".to_string(),
                    UsageOperation::Unknown(err) => err.to_owned(),
                };

                let (status_str, failure_cause) = match event.status() {
                    UsageStatus::Failed(err) => ("failed".to_string(), Some(err)),
                    UsageStatus::Success => ("success".to_string(), None),
                };

                let processing_time_ms = event.processing_time().as_millis() as i64;

                if let Err(e) = sqlx::query(
                    "INSERT INTO usage_records (client_id, operation, status, failure_cause, processing_time_ms) VALUES (?, ?, ?, ?, ?)"
                )
                .bind(id)
                .bind(operation_str)
                .bind(status_str)
                .bind(failure_cause)
                .bind(processing_time_ms)
                .execute(&pool)
                .await {
                    eprintln!("{}", e)
                }
            }
        });

        Self { sender: tx }
    }
}

pub struct PostgresUsageRecorder {
    sender: Sender<UsageEvent>,
}

impl UsageRecorder for PostgresUsageRecorder {
    fn record(&self, event: UsageEvent) {
        if let Err(e) = self.sender.try_send(event) {
            eprintln!("{:?}", e);
        };
    }
}

impl PostgresUsageRecorder {
    pub fn new(pool: PgPool) -> Self {
        let (tx, mut rx) = mpsc::channel::<UsageEvent>(1024);

        tokio::spawn(async move {
            while let Some(event) = rx.recv().await {
                let id = event.id().map(|id| id.value().to_string());

                let operation_str = match event.operation() {
                    UsageOperation::Upload => "upload".to_string(),
                    UsageOperation::Segment => "segment".to_string(),
                    UsageOperation::Save => "save".to_string(),
                    UsageOperation::Redo => "redo".to_string(),
                    UsageOperation::Undo => "undo".to_string(),
                    UsageOperation::GetPreview => "get-preview".to_string(),
                    UsageOperation::GetCompleted => "get-completed".to_string(),
                    UsageOperation::Unknown(err) => err.to_owned(),
                };

                let (status_str, failure_cause) = match event.status() {
                    UsageStatus::Failed(err) => ("failed".to_string(), Some(err)),
                    UsageStatus::Success => ("success".to_string(), None),
                };

                let processing_time_ms = event.processing_time().as_millis() as i64;

                if let Err(e) = sqlx::query(
                    "INSERT INTO usage_records (client_id, operation, status, failure_cause, processing_time_ms) VALUES ($1, $2, $3, $4, $5)"
                )
                .bind(id)
                .bind(operation_str)
                .bind(status_str)
                .bind(failure_cause)
                .bind(processing_time_ms)
                .execute(&pool)
                .await {
                    eprintln!("{}", e)
                }
            }
        });

        Self { sender: tx }
    }
}

#[cfg(test)]
mod sqlite_migration_test {
    use std::borrow::Cow;

    use sqlx::{SqlitePool, sqlite::SqlitePoolOptions};

    async fn memory_pool() -> SqlitePool {
        SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap()
    }

    async fn insert_record(pool: &SqlitePool) {
        sqlx::query(
            "INSERT INTO usage_records (client_id, operation, status, failure_cause, processing_time_ms) VALUES (?, ?, ?, ?, ?)",
        )
        .bind(Some("client"))
        .bind("segment")
        .bind("success")
        .bind(None::<String>)
        .bind(12_i64)
        .execute(pool)
        .await
        .unwrap();
    }

    // SQLite でも usage_records.id が自動採番されること（NULL にならないこと）。
    #[tokio::test]
    async fn sqlite_migrations_assign_ids_to_new_records() {
        let pool = memory_pool().await;
        sqlx::migrate!("./migrations/sqlite").run(&pool).await.unwrap();

        insert_record(&pool).await;
        insert_record(&pool).await;

        let ids: Vec<Option<i64>> = sqlx::query_scalar("SELECT id FROM usage_records ORDER BY rowid")
            .fetch_all(&pool)
            .await
            .unwrap();
        assert_eq!(ids, vec![Some(1), Some(2)]);
    }

    // 旧マイグレーション（2本）だけが適用済みで id が NULL の行がある既存 DB を、
    // 新しいマイグレーションで更新すると、既存行が残ったまま id が振られること。
    #[tokio::test]
    async fn sqlite_migrations_upgrade_existing_database_and_assign_ids() {
        let pool = memory_pool().await;
        let mut legacy = sqlx::migrate!("./migrations/sqlite");
        legacy.migrations = Cow::Owned(legacy.migrations[..2].to_vec());
        legacy.run(&pool).await.unwrap();
        insert_record(&pool).await;
        insert_record(&pool).await;

        sqlx::migrate!("./migrations/sqlite").run(&pool).await.unwrap();
        insert_record(&pool).await;

        let rows: Vec<(Option<i64>, Option<String>, String)> =
            sqlx::query_as("SELECT id, client_id, operation FROM usage_records ORDER BY id")
                .fetch_all(&pool)
                .await
                .unwrap();
        assert_eq!(
            rows,
            vec![
                (Some(1), Some("client".to_string()), "segment".to_string()),
                (Some(2), Some("client".to_string()), "segment".to_string()),
                (Some(3), Some("client".to_string()), "segment".to_string()),
            ]
        );
    }
}
