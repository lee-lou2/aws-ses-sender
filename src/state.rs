//! Application state shared across handlers

use crate::models::request::EmailRequest;
use sqlx::SqlitePool;
use tokio::sync::mpsc;

/// Shared application state accessible via Axum's State extractor.
#[derive(Clone)]
pub struct AppState {
    pub db_pool: SqlitePool,
    pub tx: mpsc::Sender<EmailRequest>,
}

impl AppState {
    #[must_use]
    pub const fn new(db_pool: SqlitePool, tx: mpsc::Sender<EmailRequest>) -> Self {
        Self { db_pool, tx }
    }
}
