//! Test modules and shared helpers

mod auth_tests;
mod event_tests;
mod handler_tests;
mod health_tests;
mod request_tests;
mod scheduler_tests;
mod status_tests;
mod topic_tests;

#[cfg(test)]
pub mod helpers {
    use sqlx::{sqlite::SqlitePoolOptions, SqlitePool};

    use crate::models::{
        content::EmailContent,
        request::{EmailMessageStatus, EmailRequest},
    };

    pub async fn setup_db() -> SqlitePool {
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap();

        // email_contents 테이블
        sqlx::query(
            "CREATE TABLE email_contents (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                subject VARCHAR(255) NOT NULL,
                content TEXT NOT NULL,
                created_at DATETIME NOT NULL DEFAULT (datetime('now'))
            )",
        )
        .execute(&pool)
        .await
        .unwrap();

        // email_requests 테이블 (content_id FK 참조)
        sqlx::query(
            "CREATE TABLE email_requests (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                topic_id VARCHAR(255) NOT NULL,
                content_id INTEGER NOT NULL,
                message_id VARCHAR(255),
                email VARCHAR(255) NOT NULL,
                scheduled_at DATETIME NOT NULL,
                status TINYINT NOT NULL DEFAULT 0,
                error VARCHAR(255),
                created_at DATETIME NOT NULL DEFAULT (datetime('now')),
                updated_at DATETIME NOT NULL DEFAULT (datetime('now')),
                deleted_at DATETIME,
                FOREIGN KEY (content_id) REFERENCES email_contents(id)
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

        // 개별 인덱스
        sqlx::query("CREATE INDEX idx_requests_topic_id ON email_requests(topic_id)")
            .execute(&pool)
            .await
            .unwrap();

        sqlx::query("CREATE INDEX idx_requests_content_id ON email_requests(content_id)")
            .execute(&pool)
            .await
            .unwrap();

        sqlx::query("CREATE INDEX idx_requests_message_id ON email_requests(message_id)")
            .execute(&pool)
            .await
            .unwrap();

        // 복합 인덱스: 스케줄러 쿼리 최적화
        sqlx::query(
            "CREATE INDEX idx_requests_status_scheduled ON email_requests(status, scheduled_at)",
        )
        .execute(&pool)
        .await
        .unwrap();

        // 복합 인덱스: 발송 건수 조회 최적화
        sqlx::query(
            "CREATE INDEX idx_requests_status_created ON email_requests(status, created_at)",
        )
        .execute(&pool)
        .await
        .unwrap();

        // 복합 인덱스: stop_topic 쿼리 최적화
        sqlx::query("CREATE INDEX idx_requests_status_topic ON email_requests(status, topic_id)")
            .execute(&pool)
            .await
            .unwrap();

        // email_results 인덱스
        sqlx::query("CREATE INDEX idx_results_request_id ON email_results(request_id)")
            .execute(&pool)
            .await
            .unwrap();

        sqlx::query("CREATE INDEX idx_results_status ON email_results(status)")
            .execute(&pool)
            .await
            .unwrap();

        pool
    }

    pub fn get_api_key() -> String {
        crate::config::get_environments().api_key.clone()
    }

    pub fn create_test_content() -> EmailContent {
        EmailContent {
            id: None,
            subject: "Test Subject".to_string(),
            content: "<p>Test Content</p>".to_string(),
        }
    }

    pub fn create_test_request_with_content_id(content_id: i32) -> EmailRequest {
        EmailRequest {
            id: None,
            topic_id: Some("test_topic".to_string()),
            content_id: Some(content_id),
            email: "test@example.com".to_string(),
            subject: String::new(),
            content: String::new(),
            scheduled_at: None,
            status: EmailMessageStatus::Created as i32,
            error: None,
            message_id: None,
        }
    }

    /// Creates a test request with content already saved to DB.
    #[allow(dead_code)]
    pub async fn create_test_request_with_db(db: &SqlitePool) -> EmailRequest {
        let content = create_test_content().save(db).await.unwrap();
        create_test_request_with_content_id(content.id.unwrap())
    }

    /// Inserts a default content and returns its id.
    pub async fn insert_default_content(db: &SqlitePool) -> i32 {
        let content = create_test_content().save(db).await.unwrap();
        content.id.unwrap()
    }

    /// Inserts a request with specified parameters (uses raw SQL for flexibility).
    pub async fn insert_request_raw(
        db: &SqlitePool,
        content_id: i32,
        topic_id: &str,
        email: &str,
        status: i32,
        scheduled_offset: Option<&str>,
    ) {
        let scheduled_at = scheduled_offset.unwrap_or("-1 hour");
        sqlx::query(&format!(
            "INSERT INTO email_requests (topic_id, content_id, email, scheduled_at, status)
                 VALUES (?, ?, ?, datetime('now', '{scheduled_at}'), ?)"
        ))
        .bind(topic_id)
        .bind(content_id)
        .bind(email)
        .bind(status)
        .execute(db)
        .await
        .unwrap();
    }

    /// Inserts a request with explicit id (uses raw SQL).
    pub async fn insert_request_with_id(
        db: &SqlitePool,
        id: i32,
        content_id: i32,
        topic_id: &str,
        email: &str,
        status: i32,
        message_id: Option<&str>,
    ) {
        sqlx::query(
            "INSERT INTO email_requests (id, topic_id, content_id, email, scheduled_at, status, message_id)
             VALUES (?, ?, ?, ?, datetime('now'), ?, ?)",
        )
        .bind(id)
        .bind(topic_id)
        .bind(content_id)
        .bind(email)
        .bind(status)
        .bind(message_id)
        .execute(db)
        .await
        .unwrap();
    }
}
