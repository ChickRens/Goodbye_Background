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

use std::{env, sync::Arc, time::Duration};

use presentation::router;
use sqlx::postgres::PgPoolOptions;
use tokio::net::TcpListener;

use crate::{
    composition::wiring::App,
    config::CLEANUP_INTERVAL,
    maintenance::deleting_expired_entries::clock::{clock::AppClock, clock_impl::RealClock},
};

#[tokio::main]
async fn main() {
    let database_url = env::var("DATABASE_URL").expect("DATABASE_URL must be set");

    let pool = PgPoolOptions::new()
        .max_connections(1)
        .connect(&format!("{}", database_url))
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
