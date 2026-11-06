//! Goodbye Background — SAM2 を使った背景除去 Web アプリのサーバー（エントリーポイント）。
//!
//! 起動の流れ:
//! 1. コマンドライン引数で利用ログの保存先 DB を選ぶ（なし/`--sqlite` → SQLite, `--postgres` → PostgreSQL）
//! 2. DB に接続してマイグレーションを実行し、`UsageRecorder` と動画の回数制限（`DailyQuota`）を同じプールで作る
//! 3. `App::new` で画像の全ユースケースを組み立てる（ここで `models/` の ONNX モデルを読み込む。無いと panic）
//! 4. `VideoCutoutApp::new` で動画のユースケースを組み立てる（動画の環境変数が足りなければ無効 = 503）
//! 5. 期限切れデータ削除（GC）タスクを `CLEANUP_INTERVAL` ごとに回す（画像のストアと、補助として動画のジョブ）
//! 6. axum でポート `PORT`（デフォルト 3000）を listen。API 以外のパスは `frontend/` を静的配信。
//!    利用者の IP を取るため `into_make_service_with_connect_info` で serve する
//!
//! 必要な環境変数: `DATABASE_URL_SQLITE` または `DATABASE_URL_POSTGRES`、任意で `PORT`。
//! 動画切り抜きの環境変数は `.env.example` と `video_cutout::config` を参照。
//! カレントディレクトリはリポジトリ直下である必要がある（`models/` `frontend/` を相対パスで参照するため）。
//!
//! モジュール構成の全体像はリポジトリ直下の `CLAUDE.md` / `docs/ARCHITECTURE.md` を参照。

mod analytics;
mod api;
mod application;
mod common;
mod composition;
mod config;
mod image_cutout;
mod maintenance;
mod presentation;
mod video_cutout;

use std::{env, net::SocketAddr, process, sync::Arc, time::Duration};

use presentation::router;
use sqlx::{postgres::PgPoolOptions, sqlite::SqlitePoolOptions};
use tokio::net::TcpListener;

use crate::{
    analytics::usage_tracking::{
        recorder::{PostgresUsageRecorder, SQLiteUsageRecorder},
        usage_recorder::UsageRecorder,
    },
    composition::{video_wiring::VideoCutoutApp, wiring::App},
    config::CLEANUP_INTERVAL,
    maintenance::deleting_expired_entries::clock::{clock::AppClock, clock_impl::RealClock},
    video_cutout::{
        config::VideoCutoutConfig,
        quota::{
            daily_quota::DailyQuota, daily_quota_postgres::PostgresDailyQuota,
            daily_quota_sqlite::SqliteDailyQuota,
        },
    },
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
    
    let (usage_recorder, video_quota): (Arc<dyn UsageRecorder>, Arc<dyn DailyQuota>) = match backend {
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
            (
                Arc::new(SQLiteUsageRecorder::new(pool.clone())),
                Arc::new(SqliteDailyQuota::new(pool)),
            )
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
            (
                Arc::new(PostgresUsageRecorder::new(pool.clone())),
                Arc::new(PostgresDailyQuota::new(pool)),
            )
        }
    };

    // ONNX モデル（models/*.onnx, models/image_pe.bin）の読み込みもここで行われる。
    let app = Arc::new(App::new(usage_recorder).unwrap());
    let clock = RealClock::new();

    let video = Arc::new(VideoCutoutApp::new(video_quota, VideoCutoutConfig::from_env()));
    if video.is_enabled() {
        println!("動画切り抜き: 有効");
        if video.log_client_ip_debug() {
            // 一時的な確認用。TRUSTED_PROXY_HOPS を決めたら LOG_CLIENT_IP_DEBUG を外す
            println!("動画切り抜き: IP の確認用ログ（[client-ip-debug]）を出します");
        }
    } else {
        println!("動画切り抜き: 無効（環境変数が足りないため、動画の API は 503 を返す）");
    }

    let app_for_gc = app.clone();
    let app_for_router = app.clone();

    // 60 秒ごとに、全インメモリストアから「最後のアクセスから TTL_SEC 以上経った」エントリを消す。
    spawn_gc_task(CLEANUP_INTERVAL, move || {
        app_for_gc.cleanup(clock.now());
    });

    // 動画のジョブは動画 API のリクエスト処理中にも片付ける（Cloud Run では裏のタスクがほとんど進まないため）。
    // こちらは補助。非同期なので画像の GC（同期のクロージャ）とは別のタスクで回す
    let video_for_gc = video.clone();
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(CLEANUP_INTERVAL);
        // 片付けが長引いたり CPU が止まっていた（Cloud Run）後に、溜まった tick を連続で撃たない
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            interval.tick().await;
            video_for_gc.cleanup_video_jobs().await;
        }
    });

    let router = router::route(app_for_router.clone(), video);

    let port: u16 = env::var("PORT")
        .unwrap_or_else(|_| "3000".to_string())
        .parse()
        .expect("PORT must be a valid port number");

    let listener = TcpListener::bind(("0.0.0.0", port))
        .await
        .expect("Failed to bind TCP listener");

    println!("Server running on 0.0.0.0:{port}");

    axum::serve(
        listener,
        router.into_make_service_with_connect_info::<SocketAddr>(),
    )
    .await
    .unwrap();
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
