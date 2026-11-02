//! Goodbye Background — SAM2 を使った背景除去 Web アプリのサーバー（エントリーポイント）。
//!
//! 起動の流れ:
//! 1. コマンドライン引数で利用ログの保存先 DB を選ぶ（なし/`--sqlite` → SQLite, `--postgres` → PostgreSQL）
//! 2. DB に接続して `UsageRecorder` を作る（SQLite のときだけマイグレーションを実行）
//! 3. `App::new` で全ユースケースを組み立てる（ここで `models/` の ONNX モデルを読み込む。無いと panic）
//! 4. 期限切れデータ削除（GC）タスクを `CLEANUP_INTERVAL` ごとに回す
//! 5. axum でポート `PORT`（デフォルト 3000）を listen。API 以外のパスは `frontend/` を静的配信
//!
//! 必要な環境変数: `DATABASE_URL_SQLITE` または `DATABASE_URL_POSTGRES`、任意で `PORT`。
//! カレントディレクトリはリポジトリ直下である必要がある（`models/` `frontend/` を相対パスで参照するため）。
//!
//! モジュール構成の全体像はリポジトリ直下の `CLAUDE.md` / `docs/ARCHITECTURE.md` を参照。

mod analytics;
mod api;
mod application;
mod common;
mod composition;
mod config;
mod editing;
mod image;
mod image_io;
mod maintenance;
mod point;
mod presentation;
mod session;

use std::{env, process, sync::Arc, time::Duration};

use presentation::router;
use sqlx::{postgres::PgPoolOptions, sqlite::SqlitePoolOptions};
use tokio::net::TcpListener;

use crate::{
    analytics::usage_tracking::{
        recorder::{PostgresUsageRecorder, SQLiteUsageRecorder},
        usage_recorder::UsageRecorder,
    },
    composition::wiring::App,
    config::CLEANUP_INTERVAL,
    maintenance::deleting_expired_entries::clock::{clock::AppClock, clock_impl::RealClock},
};

#[tokio::main]
async fn main() {
    let backend = match database_backend() {
        Ok(backend) => backend,
        Err(message) => {
            eprintln!("{message}");
            process::exit(2);
        }
    };
    
    let usage_recorder: Arc<dyn UsageRecorder> = match backend {
        DatabaseBackend::Sqlite => {
            let database_url = env::var("DATABASE_URL_SQLITE").expect("DATABASE_URL_SQLITE must be set");
            let pool = SqlitePoolOptions::new()
                .max_connections(1)
                .connect(&database_url)
                .await
                .expect("Failed to connect to SQLite database");
            sqlx::migrate!("./migrations/sqlite")
                .run(&pool)
                .await
                .expect("Failed to run SQLite migrations");
            Arc::new(SQLiteUsageRecorder::new(pool))
        }
        DatabaseBackend::Postgres => {
            let database_url = env::var("DATABASE_URL_POSTGRES").expect("DATABASE_URL_POSTGRES must be set");
            let pool = PgPoolOptions::new()
                .max_connections(1)
                .connect(&database_url)
                .await
                .expect("Failed to connect to PostgreSQL database");
            // SQLite と Postgres で型が違うので、マイグレーションはディレクトリを分けている。
            // 手動でテーブルを作った既存 DB でも通るよう、Postgres 側は IF NOT EXISTS で書いてある。
            sqlx::migrate!("./migrations/postgres")
                .run(&pool)
                .await
                .expect("Failed to run PostgreSQL migrations");
            Arc::new(PostgresUsageRecorder::new(pool))
        }
    };

    // ONNX モデル（models/*.onnx, models/image_pe.bin）の読み込みもここで行われる。
    let app = Arc::new(App::new(usage_recorder).unwrap());
    let clock = RealClock::new();

    let app_for_gc = app.clone();
    let app_for_router = app.clone();

    // 60 秒ごとに、全インメモリストアから「最後のアクセスから TTL_SEC 以上経った」エントリを消す。
    spawn_gc_task(CLEANUP_INTERVAL, move || {
        app_for_gc.cleanup(clock.now());
    });

    let router = router::route(app_for_router.clone());

    let port: u16 = env::var("PORT")
        .unwrap_or_else(|_| "3000".to_string())
        .parse()
        .expect("PORT must be a valid port number");

    let listener = TcpListener::bind(("0.0.0.0", port))
        .await
        .expect("Failed to bind TCP listener");

    println!("Server running on 0.0.0.0:{port}");

    axum::serve(listener, router).await.unwrap();
}

/// 利用ログ（usage_records テーブル）の保存先。画像データ自体は DB に入らず、全てメモリ上に置かれる。
enum DatabaseBackend {
    Postgres,
    Sqlite,
}

fn database_backend() -> Result<DatabaseBackend, String> {
    let args: Vec<String> = env::args().skip(1).collect();
    match args.as_slice() {
        [] => Ok(DatabaseBackend::Sqlite),
        [arg] if arg == "--sqlite" => Ok(DatabaseBackend::Sqlite),
        [arg] if arg == "--postgres" => Ok(DatabaseBackend::Postgres),
        _ => Err("Usage: cargo run -- [--sqlite | --postgres]".to_string()),
    }
}

/// `interval` ごとに `task` を実行し続けるバックグラウンドタスクを起動する。
///
/// 注意: `task` は tokio のワーカースレッド上で同期実行される（`spawn_blocking` ではない）。
/// ストアが大きいと `DashMap::retain` の間ワーカーを占有する。
fn spawn_gc_task<F>(interval: Duration, task: F)
where
    F: Fn() + Send + Sync + 'static,
{
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(interval);
        loop {
            interval.tick().await;
            task()
        }
    });
}
