//! 애플리케이션 상태 모듈.

use sqlx::SqlitePool;
use tokio::sync::mpsc;

use crate::models::request::EmailRequest;

/// Shared application state accessible via Axum's State extractor.
#[derive(Clone)]
pub struct AppState {
    /// `SQLite` connection pool
    pub db_pool: SqlitePool,
    /// Channel sender for email requests
    pub tx: mpsc::Sender<EmailRequest>,
}

impl AppState {
    /// Creates a new `AppState` instance.
    #[must_use]
    pub const fn new(db_pool: SqlitePool, tx: mpsc::Sender<EmailRequest>) -> Self {
        Self { db_pool, tx }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_app_state_is_clone() {
        fn assert_clone<T: Clone>() {}
        assert_clone::<AppState>();
    }

    #[test]
    fn test_app_state_struct_size() {
        let size = std::mem::size_of::<AppState>();
        assert!(size > 0);
        assert!(size < 256);
    }
}
