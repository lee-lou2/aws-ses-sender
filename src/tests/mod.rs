//! Test modules

mod auth_tests;
mod event_tests;
mod handler_tests;
mod request_tests;
mod status_tests;

#[cfg(test)]
pub mod helpers {
    use sqlx::{sqlite::SqlitePoolOptions, SqlitePool};

    pub async fn setup_db() -> SqlitePool {
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap();

        sqlx::query(
            "CREATE TABLE email_requests (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                topic_id VARCHAR(255) NOT NULL,
                message_id VARCHAR(255),
                email VARCHAR(255) NOT NULL,
                subject VARCHAR(255) NOT NULL,
                content TEXT NOT NULL,
                scheduled_at DATETIME NOT NULL,
                status TINYINT NOT NULL DEFAULT 0,
                error VARCHAR(255),
                created_at DATETIME NOT NULL DEFAULT (datetime('now')),
                updated_at DATETIME NOT NULL DEFAULT (datetime('now')),
                deleted_at DATETIME
            )",
        )
        .execute(&pool)
        .await
        .unwrap();

        sqlx::query(
            "CREATE TABLE email_results (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                request_id INTEGER NOT NULL,
                status VARCHAR(50) NOT NULL,
                raw TEXT,
                created_at DATETIME NOT NULL DEFAULT (datetime('now')),
                FOREIGN KEY (request_id) REFERENCES email_requests(id)
            )",
        )
        .execute(&pool)
        .await
        .unwrap();

        sqlx::query("CREATE INDEX idx_requests_status ON email_requests(status)")
            .execute(&pool)
            .await
            .unwrap();

        sqlx::query("CREATE INDEX idx_requests_topic_id ON email_requests(topic_id)")
            .execute(&pool)
            .await
            .unwrap();

        pool
    }

    pub fn get_api_key() -> String {
        crate::config::get_environments().api_key.clone()
    }
}
