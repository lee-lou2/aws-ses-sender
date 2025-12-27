//! High-performance bulk email service via AWS SES

mod app;
mod config;
mod handlers;
mod middlewares;
mod models;
mod services;
mod state;
mod tests;

use std::env;

use tracing::info;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt, EnvFilter};

use crate::services::receiver::{receive_post_send_message, receive_send_message};
use crate::services::scheduler::schedule_pre_send_message;

// Database pool configuration
const DB_MAX_CONNECTIONS: u32 = 20;
const DB_MIN_CONNECTIONS: u32 = 5;
const DB_ACQUIRE_TIMEOUT_SECS: u64 = 30;
const DB_IDLE_TIMEOUT_SECS: u64 = 300;

// Channel buffer sizes
const SEND_CHANNEL_BUFFER: usize = 10_000;
const POST_SEND_CHANNEL_BUFFER: usize = 1_000;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    init_logger();
    info!("Starting aws-ses-sender...");

    let envs = config::get_environments();
    let _sentry_guard = init_sentry(&envs.sentry_dsn);

    let db_pool = init_database().await?;
    let (tx_send, rx_send) = tokio::sync::mpsc::channel(SEND_CHANNEL_BUFFER);
    let (tx_post_send, rx_post_send) = tokio::sync::mpsc::channel(POST_SEND_CHANNEL_BUFFER);

    spawn_scheduler(tx_send.clone(), db_pool.clone());
    spawn_email_sender(rx_send, tx_post_send);
    spawn_post_processor(rx_post_send, db_pool.clone());

    let state = state::AppState::new(db_pool, tx_send);
    let app = app::app(state);

    let port = &envs.server_port;
    let listener = tokio::net::TcpListener::bind(format!("0.0.0.0:{port}")).await?;
    info!("Server running on http://0.0.0.0:{port}");
    info!(
        "Config: max_send/sec={}, db_pool={DB_MAX_CONNECTIONS}",
        envs.max_send_per_second
    );

    axum::serve(listener, app).await?;
    Ok(())
}

fn init_logger() {
    let log_level = env::var("RUST_LOG").unwrap_or_else(|_| "info".to_owned());
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::fmt::layer()
                .with_target(true)
                .with_level(true)
                .with_thread_ids(true),
        )
        .with(EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(&log_level)))
        .init();
}

fn init_sentry(dsn: &str) -> sentry::ClientInitGuard {
    sentry::init((
        dsn,
        sentry::ClientOptions {
            release: sentry::release_name!(),
            ..Default::default()
        },
    ))
}

async fn init_database() -> Result<sqlx::SqlitePool, Box<dyn std::error::Error>> {
    let pool = sqlx::sqlite::SqlitePoolOptions::new()
        .max_connections(DB_MAX_CONNECTIONS)
        .min_connections(DB_MIN_CONNECTIONS)
        .acquire_timeout(std::time::Duration::from_secs(DB_ACQUIRE_TIMEOUT_SECS))
        .idle_timeout(std::time::Duration::from_secs(DB_IDLE_TIMEOUT_SECS))
        .connect("sqlite://sqlite3.db?mode=rwc")
        .await?;

    apply_sqlite_optimizations(&pool).await?;
    Ok(pool)
}

async fn apply_sqlite_optimizations(pool: &sqlx::SqlitePool) -> Result<(), sqlx::Error> {
    // Journal and sync settings
    sqlx::query("PRAGMA journal_mode=WAL").execute(pool).await?;
    sqlx::query("PRAGMA synchronous=NORMAL")
        .execute(pool)
        .await?;
    sqlx::query("PRAGMA busy_timeout=5000")
        .execute(pool)
        .await?;

    // Memory and cache settings
    sqlx::query("PRAGMA mmap_size=268435456")
        .execute(pool)
        .await?;
    sqlx::query("PRAGMA cache_size=-64000")
        .execute(pool)
        .await?;
    sqlx::query("PRAGMA temp_store=MEMORY")
        .execute(pool)
        .await?;

    // Storage optimization
    sqlx::query("PRAGMA page_size=4096").execute(pool).await?;
    sqlx::query("PRAGMA auto_vacuum=INCREMENTAL")
        .execute(pool)
        .await?;

    // Integrity
    sqlx::query("PRAGMA foreign_keys=ON").execute(pool).await?;

    info!("SQLite optimized: WAL, mmap=256MB, cache=64MB, temp=MEMORY");
    Ok(())
}

fn spawn_scheduler(
    tx: tokio::sync::mpsc::Sender<models::request::EmailRequest>,
    db: sqlx::SqlitePool,
) {
    tokio::spawn(async move {
        schedule_pre_send_message(&tx, db).await;
    });
}

fn spawn_email_sender(
    rx: tokio::sync::mpsc::Receiver<models::request::EmailRequest>,
    tx: tokio::sync::mpsc::Sender<models::request::EmailRequest>,
) {
    tokio::spawn(async move {
        receive_send_message(rx, tx).await;
    });
}

fn spawn_post_processor(
    rx: tokio::sync::mpsc::Receiver<models::request::EmailRequest>,
    db: sqlx::SqlitePool,
) {
    tokio::spawn(async move {
        receive_post_send_message(rx, db).await;
    });
}
