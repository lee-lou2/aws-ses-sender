mod app;
mod config;
mod handlers;
mod middlewares;
mod models;
mod services;
mod state;
mod tests;

use aws_config::meta::region::RegionProviderChain;
use aws_config::BehaviorVersion;
use aws_sdk_sesv2::{config::Region, Client};
use services::receiver::{receive_post_send_message, receive_send_message};
use services::scheduler::schedule_pre_send_message;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

#[tokio::main]
async fn main() -> Result<(), sqlx::Error> {
    let envs = config::get_environments();

    // Sentry Initialization
    let sentry_dsn = &envs.sentry_dsn;
    let _guard = sentry::init((
        sentry_dsn.as_str(),
        sentry::ClientOptions {
            release: sentry::release_name!(),
            ..Default::default()
        },
    ));

    // Initialize DB
    let db_pool = sqlx::sqlite::SqlitePoolOptions::new()
        .max_connections(10)
        .connect("sqlite://sqlite3.db")
        .await
        .expect("Failed to create pool");

    // Initialize AWS SES Client
    let aws_region = &envs.aws_region;
    let region_provider = RegionProviderChain::first_try(Region::new(aws_region.clone()))
        .or_default_provider()
        .or_else(Region::new(aws_region.clone()));

    let shared_config = aws_config::defaults(BehaviorVersion::latest())
        .region(region_provider)
        .load()
        .await;
    let client = Client::new(&shared_config);

    // Initialize channels
    let (tx_send, rx_send) = tokio::sync::mpsc::channel(10000);
    let (tx_post_send, rx_post_send) = tokio::sync::mpsc::channel(1000);
    let cloned_tx_send = tx_send.clone();

    // Preprocess email sending
    tokio::spawn({
        let db_pool = db_pool.clone();
        async move {
            schedule_pre_send_message(&tx_send, db_pool).await;
        }
    });

    // Email sending
    tokio::spawn({
        async move {
            receive_send_message(client, rx_send, tx_post_send).await;
        }
    });

    // Postprocess email sending
    tokio::spawn({
        let db_pool = db_pool.clone();
        async move {
            receive_post_send_message(rx_post_send, db_pool).await;
        }
    });

    let state = state::AppState::new(db_pool, cloned_tx_send);

    // Initialize logger
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::fmt::layer()
                .with_target(true)
                .with_level(true),
        )
        .with(tracing_subscriber::filter::LevelFilter::DEBUG)
        .init();

    let app = app::app(state).await?;

    // Start the server
    let port = &envs.server_port;
    let listener = tokio::net::TcpListener::bind(format!("0.0.0.0:{}", port)).await?;
    println!("Server running on http://0.0.0.0:{}", port);
    axum::serve(listener, app).await?;
    Ok(())
}
