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
            sqlx::migrate!("./migrations")
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
            Arc::new(PostgresUsageRecorder::new(pool))
        }
    };

    let app = Arc::new(App::new(usage_recorder).unwrap());
    let clock = RealClock::new();

    let app_for_gc = app.clone();
    let app_for_router = app.clone();

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
