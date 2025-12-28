//! High-performance bulk email service via AWS SES

mod app;
mod config;
mod constants;
mod error;
mod handlers;
mod middlewares;
mod models;
mod services;
mod state;

// Note: Tests are now inline in each module (tests/ directory can be removed)

use tokio::signal;
use tracing::info;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt, EnvFilter};

use crate::config::{close_db, init_db, APP_CONFIG};
use crate::services::receiver::{receive_post_send_message, receive_send_message};
use crate::services::scheduler::schedule_pre_send_message;

// High-performance memory allocator for non-MSVC targets
#[cfg(not(target_env = "msvc"))]
#[global_allocator]
static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    init_logger();
    info!("Starting aws-ses-sender...");

    let _sentry_guard = init_sentry();

    let db_pool = init_db().await?;
    let (tx_send, rx_send) = tokio::sync::mpsc::channel(APP_CONFIG.send_channel_buffer);
    let (tx_post_send, rx_post_send) =
        tokio::sync::mpsc::channel(APP_CONFIG.post_send_channel_buffer);

    spawn_scheduler(tx_send.clone(), db_pool.clone());
    spawn_email_sender(rx_send, tx_post_send);
    spawn_post_processor(rx_post_send, db_pool.clone());

    let state = state::AppState::new(db_pool, tx_send);
    let app = app::app(state);

    let port = &APP_CONFIG.server_port;
    let listener = tokio::net::TcpListener::bind(format!("0.0.0.0:{port}")).await?;
    info!("Server running on http://0.0.0.0:{port}");
    info!(
        "Config: max_send/sec={}, db_pool={}",
        APP_CONFIG.max_send_per_second, APP_CONFIG.db_max_connections
    );

    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await?;

    // Cleanup
    info!("Shutting down...");
    close_db().await;

    // Flush Sentry events before exit
    if let Some(client) = sentry::Hub::current().client() {
        client.flush(Some(std::time::Duration::from_secs(2)));
    }

    info!("Server shutdown complete");
    Ok(())
}

async fn shutdown_signal() {
    let ctrl_c = async {
        signal::ctrl_c()
            .await
            .expect("failed to install Ctrl+C handler");
    };

    #[cfg(unix)]
    let terminate = async {
        signal::unix::signal(signal::unix::SignalKind::terminate())
            .expect("failed to install signal handler")
            .recv()
            .await;
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        () = ctrl_c => info!("Received Ctrl+C, initiating graceful shutdown..."),
        () = terminate => info!("Received SIGTERM, initiating graceful shutdown..."),
    }
}

fn init_logger() {
    let log_level = std::env::var("RUST_LOG").unwrap_or_else(|_| "info".to_owned());
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

fn init_sentry() -> sentry::ClientInitGuard {
    sentry::init((
        APP_CONFIG.sentry_dsn.as_str(),
        sentry::ClientOptions {
            release: sentry::release_name!(),
            traces_sample_rate: APP_CONFIG.sentry_traces_sample_rate,
            sample_rate: 1.0,
            ..Default::default()
        },
    ))
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
