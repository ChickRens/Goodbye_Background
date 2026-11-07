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
                    UsageOperation::CreateVideoJob => "create-video-job".to_string(),
                    UsageOperation::StartVideoJob => "start-video-job".to_string(),
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
                    UsageOperation::CreateVideoJob => "create-video-job".to_string(),
                    UsageOperation::StartVideoJob => "start-video-job".to_string(),
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

    // Google ログインのブランチ（feat/user-google-login）で動かしていた開発用 DB
    // （20260919032531 / 20261004122625 / 20261101090000 が適用済み）を、このブランチの
    // マイグレーションで更新できること。ログインのテーブルは消え、利用ログは残る。
    #[tokio::test]
    async fn sqlite_migrations_upgrade_google_login_branch_database_and_drop_login_tables() {
        let pool = memory_pool().await;
        let mut google_branch = sqlx::migrate!("./migrations/sqlite");
        google_branch.migrations = Cow::Owned(
            google_branch
                .migrations
                .iter()
                .filter(|m| [20260919032531, 20261004122625, 20261101090000].contains(&m.version))
                .cloned()
                .collect(),
        );
        assert_eq!(google_branch.migrations.len(), 3);
        google_branch.run(&pool).await.unwrap();
        insert_record(&pool).await;
        sqlx::query("INSERT INTO users (id, google_sub, email) VALUES ('u1', 'sub', 'a@example.com')")
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query("INSERT INTO login_sessions (token_hash, user_id, expires_at) VALUES ('h', 'u1', 0)")
            .execute(&pool)
            .await
            .unwrap();

        sqlx::migrate!("./migrations/sqlite").run(&pool).await.unwrap();

        let login_tables: Vec<String> = sqlx::query_scalar(
            "SELECT name FROM sqlite_master WHERE name IN ('users', 'login_sessions', 'idx_login_sessions_expires_at')",
        )
        .fetch_all(&pool)
        .await
        .unwrap();
        assert!(login_tables.is_empty(), "残っている: {login_tables:?}");
        let ids: Vec<Option<i64>> = sqlx::query_scalar("SELECT id FROM usage_records")
            .fetch_all(&pool)
            .await
            .unwrap();
        assert_eq!(ids, vec![Some(1)]);
    }
}

// 本番の PostgreSQL は feat/user-google-login ブランチの migrations/ で作られた。
// 適用済みのファイルのチェックサムが変わると起動しなくなるので、本番が適用したときの値に固定する。
#[cfg(test)]
mod postgres_migration_checksum_test {
    #[test]
    fn postgres_migrations_applied_in_production_keep_their_checksums() {
        let expected = [
            (20260919032531_i64, "6db901ba69bd19fa05bfb784ee17d914a6c031a8d603c44d88a56e01c292b7dd8cf69c659a66a4306c3c914e9ae77186"),
            (20261004122625, "fce80b3766c787fce6fcd9203eea21a3b13e9989e89ae1daf5cad3a470f412b8bd4f318dcd22184818df3d86d0536c52"),
            (20261101090000, "d7cbb94d73748d77f08c55249b9401e2341af4f92c4cf5f2c3f7d6337e56c2111155a58cbf83ed3a56f5da069ffede63"),
        ];
        let migrator = sqlx::migrate!("./migrations/postgres");
        for (version, checksum) in expected {
            let m = migrator
                .iter()
                .find(|m| m.version == version)
                .unwrap_or_else(|| panic!("{version} が無い"));
            let actual: String = m.checksum.iter().map(|b| format!("{b:02x}")).collect();
            assert_eq!(actual, checksum, "{version} のチェックサムが本番と違う");
        }
    }
}
