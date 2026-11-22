//! `VisitRecorder` の実装。`UsageRecorder`（`usage_tracking/recorder.rs`）と同じ仕組み:
//! 容量 1024 の mpsc チャネルと、受信して INSERT し続ける tokio タスクを作る。
//! `record()` は `try_send` するだけなので、DB が遅くてもレスポンスは遅れない（満杯なら eprintln して捨てる）。
//!
//! SQLite 版と Postgres 版の違いはプレースホルダ（`?` / `$1`）と bind する型だけなので、
//! チャネルとタスクは `ChannelVisitRecorder::spawn` にまとめ、DB ごとに INSERT だけを渡す。
//! `new()` 内で `tokio::spawn` するため、tokio ランタイム内で呼ぶこと。

use std::future::Future;

use sqlx::{PgPool, SqlitePool};
use tokio::sync::mpsc::{self, Sender};

use crate::analytics::visit_tracking::{event::VisitEvent, visit_recorder::VisitRecorder};

const CHANNEL_CAPACITY: usize = 1024;

pub struct ChannelVisitRecorder {
    sender: Sender<VisitEvent>,
}

impl VisitRecorder for ChannelVisitRecorder {
    fn record(&self, event: VisitEvent) {
        if let Err(e) = self.sender.try_send(event) {
            eprintln!("{:?}", e);
        }
    }
}

impl ChannelVisitRecorder {
    fn spawn<F, Fut>(insert: F) -> Self
    where
        F: Fn(VisitEvent) -> Fut + Send + 'static,
        Fut: Future<Output = Result<(), sqlx::Error>> + Send,
    {
        let (tx, mut rx) = mpsc::channel::<VisitEvent>(CHANNEL_CAPACITY);
        tokio::spawn(async move {
            while let Some(event) = rx.recv().await {
                if let Err(e) = insert(event).await {
                    eprintln!("{}", e);
                }
            }
        });
        Self { sender: tx }
    }

    pub fn sqlite(pool: SqlitePool) -> Self {
        Self::spawn(move |event| {
            let pool = pool.clone();
            async move {
                sqlx::query(
                    "INSERT INTO visit_records (client_id, utm_source, utm_medium, utm_campaign, utm_content, landing_path) VALUES (?, ?, ?, ?, ?, ?)",
                )
                .bind(event.client_id().map(|id| id.to_string()))
                .bind(event.utm_source())
                .bind(event.utm_medium())
                .bind(event.utm_campaign())
                .bind(event.utm_content())
                .bind(event.landing_path())
                .execute(&pool)
                .await
                .map(|_| ())
            }
        })
    }

    pub fn postgres(pool: PgPool) -> Self {
        Self::spawn(move |event| {
            let pool = pool.clone();
            async move {
                sqlx::query(
                    "INSERT INTO visit_records (client_id, utm_source, utm_medium, utm_campaign, utm_content, landing_path) VALUES ($1, $2, $3, $4, $5, $6)",
                )
                .bind(event.client_id().map(|id| id.to_string()))
                .bind(event.utm_source())
                .bind(event.utm_medium())
                .bind(event.utm_campaign())
                .bind(event.utm_content())
                .bind(event.landing_path())
                .execute(&pool)
                .await
                .map(|_| ())
            }
        })
    }
}

#[cfg(test)]
mod visit_recorder_test {
    use std::time::Duration;

    use sqlx::sqlite::SqlitePoolOptions;
    use uuid::Uuid;

    use super::ChannelVisitRecorder;
    use crate::analytics::visit_tracking::{event::VisitEvent, visit_recorder::VisitRecorder};

    type Row = (Option<String>, String, Option<String>, Option<String>, Option<String>, String);

    #[tokio::test]
    async fn sqlite_recorder_inserts_a_visit() {
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap();
        sqlx::migrate!("./migrations/sqlite").run(&pool).await.unwrap();
        let recorder = ChannelVisitRecorder::sqlite(pool.clone());
        let id = Uuid::new_v4();

        recorder.record(
            VisitEvent::new(Some(id), Some("youtube"), Some("longform"), None, None, Some("/")).unwrap(),
        );

        // INSERT は別タスクなので、入るまで最大 1 秒待つ
        let mut rows: Vec<Row> = Vec::new();
        for _ in 0..100 {
            rows = sqlx::query_as(
                "SELECT client_id, utm_source, utm_medium, utm_campaign, utm_content, landing_path FROM visit_records",
            )
            .fetch_all(&pool)
            .await
            .unwrap();
            if !rows.is_empty() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        assert_eq!(
            rows,
            vec![(Some(id.to_string()), "youtube".into(), Some("longform".into()), None, None, "/".into())]
        );
    }

    /// 手動で実行する: 空の PostgreSQL を用意して
    /// `TEST_DATABASE_URL_POSTGRES=postgres://... cargo test postgres_recorder -- --ignored`
    #[tokio::test]
    #[ignore]
    async fn postgres_recorder_inserts_a_visit() {
        let url = std::env::var("TEST_DATABASE_URL_POSTGRES")
            .expect("TEST_DATABASE_URL_POSTGRES に空の PostgreSQL を指定して実行すること");
        let pool = sqlx::postgres::PgPoolOptions::new().max_connections(1).connect(&url).await.unwrap();
        sqlx::migrate!("./migrations/postgres").run(&pool).await.unwrap();
        let recorder = ChannelVisitRecorder::postgres(pool.clone());
        let id = Uuid::new_v4();

        recorder.record(
            VisitEvent::new(Some(id), Some("youtube"), Some("longform"), Some("c"), None, Some("/video.html")).unwrap(),
        );

        let mut rows: Vec<Row> = Vec::new();
        for _ in 0..100 {
            rows = sqlx::query_as(
                "SELECT client_id, utm_source, utm_medium, utm_campaign, utm_content, landing_path FROM visit_records WHERE client_id = $1",
            )
            .bind(id.to_string())
            .fetch_all(&pool)
            .await
            .unwrap();
            if !rows.is_empty() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        assert_eq!(
            rows,
            vec![(Some(id.to_string()), "youtube".into(), Some("longform".into()), Some("c".into()), None, "/video.html".into())]
        );
    }
}
