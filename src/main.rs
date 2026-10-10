mod application;
mod common;
mod composition;
mod infrastructure;
mod presentation;
mod image_io;
mod editing;
mod api;
mod image;
mod point;
mod session;

use std::{env, sync::Arc, time::Duration};

use presentation::router;
use sqlx::sqlite::SqlitePoolOptions;
use tokio::net::TcpListener;

use crate::{
    application::{interface::clock::AppClock, usecase::config::CLEANUP_INTERVAL},
    composition::wiring::App,
    infrastructure::clock::RealClock,
};

#[tokio::main]
async fn main() {
    let database_url = env::var("DATABASE_URL").expect("DATABASE_URL must be set");

    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect(&format!("{}?mode=rwc", database_url))
        .await
        .unwrap();
    sqlx::query("PRAGMA journal_mode=WAL")
        .execute(&pool)
        .await
        .unwrap();
    sqlx::migrate!("./migrations").run(&pool).await.unwrap();

    let app = Arc::new(App::new(pool).unwrap());
    let clock = RealClock::new();

    let app_for_gc = app.clone();
    let app_for_router = app.clone();

    spawn_gc_task(CLEANUP_INTERVAL, move || {
        app_for_gc.cleanup(clock.now());
    });

    let router = router::route(app_for_router.clone());
    println!("Server running on http://localhost:3000");

    let listener = TcpListener::bind("0.0.0.0:3000").await.unwrap();
    axum::serve(listener, router).await.unwrap();
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
