//! Email result model for tracking delivery events

use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;
use std::collections::HashMap;

/// Email delivery result (Bounce, Complaint, Delivery, Open, etc.)
#[derive(Debug, Deserialize, Serialize)]
pub struct EmailResult {
    pub id: Option<i32>,
    pub request_id: i32,
    pub status: String,
    pub raw: Option<String>,
}

impl EmailResult {
    /// Saves the email result to the database.
    pub async fn save(self, db_pool: &SqlitePool) -> Result<Self, sqlx::Error> {
        let row: (i64,) = sqlx::query_as(
            "INSERT INTO email_results (request_id, status, raw, created_at) VALUES (?, ?, ?, datetime('now')) RETURNING id",
        )
        .bind(self.request_id)
        .bind(&self.status)
        .bind(&self.raw)
        .fetch_one(db_pool)
        .await?;

        #[allow(clippy::cast_possible_truncation)]
        Ok(Self {
            id: Some(row.0 as i32),
            ..self
        })
    }

    /// Returns result counts by status for the specified topic.
    pub async fn get_result_counts_by_topic_id(
        db_pool: &SqlitePool,
        topic_id: &str,
    ) -> Result<HashMap<String, i32>, sqlx::Error> {
        let rows: Vec<(String, i32)> = sqlx::query_as(
            "SELECT status, COUNT(DISTINCT request_id)
             FROM email_results
             WHERE request_id IN (SELECT id FROM email_requests WHERE topic_id = ?)
             GROUP BY status",
        )
        .bind(topic_id)
        .fetch_all(db_pool)
        .await?;

        Ok(rows.into_iter().collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::sqlite::SqlitePoolOptions;

    async fn setup_db() -> SqlitePool {
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

        pool
    }

    #[tokio::test]
    async fn test_get_result_counts() {
        let db = setup_db().await;

        sqlx::query(
            "INSERT INTO email_requests (topic_id, email, subject, content, scheduled_at) VALUES (?, ?, ?, ?, datetime('now'))",
        )
        .bind("topic1")
        .bind("test@example.com")
        .bind("subject")
        .bind("content")
        .execute(&db)
        .await
        .unwrap();

        EmailResult {
            id: None,
            request_id: 1,
            status: "success".into(),
            raw: None,
        }
        .save(&db)
        .await
        .unwrap();

        EmailResult {
            id: None,
            request_id: 1,
            status: "failed".into(),
            raw: None,
        }
        .save(&db)
        .await
        .unwrap();

        let counts = EmailResult::get_result_counts_by_topic_id(&db, "topic1")
            .await
            .unwrap();
        assert_eq!(counts.get("success"), Some(&1));
        assert_eq!(counts.get("failed"), Some(&1));
    }

    #[tokio::test]
    async fn test_empty_results() {
        let db = setup_db().await;
        let counts = EmailResult::get_result_counts_by_topic_id(&db, "nonexistent")
            .await
            .unwrap();
        assert!(counts.is_empty());
    }

    #[tokio::test]
    async fn test_multiple_results_same_request() {
        let db = setup_db().await;

        sqlx::query(
            "INSERT INTO email_requests (topic_id, email, subject, content, scheduled_at) VALUES (?, ?, ?, ?, datetime('now'))",
        )
        .bind("topic1")
        .bind("test@example.com")
        .bind("subject")
        .bind("content")
        .execute(&db)
        .await
        .unwrap();

        // Multiple events for the same request (e.g., Delivery then Open)
        EmailResult {
            id: None,
            request_id: 1,
            status: "Delivery".into(),
            raw: None,
        }
        .save(&db)
        .await
        .unwrap();

        EmailResult {
            id: None,
            request_id: 1,
            status: "Open".into(),
            raw: None,
        }
        .save(&db)
        .await
        .unwrap();

        EmailResult {
            id: None,
            request_id: 1,
            status: "Open".into(),
            raw: None,
        }
        .save(&db)
        .await
        .unwrap();

        // get_result_counts uses DISTINCT request_id
        let counts = EmailResult::get_result_counts_by_topic_id(&db, "topic1")
            .await
            .unwrap();

        assert_eq!(counts.get("Delivery"), Some(&1));
        assert_eq!(counts.get("Open"), Some(&1));
    }

    #[tokio::test]
    async fn test_result_counts_multiple_requests() {
        let db = setup_db().await;

        // Insert multiple requests
        for i in 1..=3 {
            sqlx::query(
                "INSERT INTO email_requests (topic_id, email, subject, content, scheduled_at) VALUES (?, ?, ?, ?, datetime('now'))",
            )
            .bind("multi_topic")
            .bind(format!("test{i}@example.com"))
            .bind("subject")
            .bind("content")
            .execute(&db)
            .await
            .unwrap();
        }

        // Results for each request
        EmailResult {
            id: None,
            request_id: 1,
            status: "Delivery".into(),
            raw: None,
        }
        .save(&db)
        .await
        .unwrap();

        EmailResult {
            id: None,
            request_id: 2,
            status: "Delivery".into(),
            raw: None,
        }
        .save(&db)
        .await
        .unwrap();

        EmailResult {
            id: None,
            request_id: 3,
            status: "Bounce".into(),
            raw: None,
        }
        .save(&db)
        .await
        .unwrap();

        let counts = EmailResult::get_result_counts_by_topic_id(&db, "multi_topic")
            .await
            .unwrap();

        assert_eq!(counts.get("Delivery"), Some(&2));
        assert_eq!(counts.get("Bounce"), Some(&1));
    }

    #[tokio::test]
    async fn test_save_returns_correct_id() {
        let db = setup_db().await;

        sqlx::query(
            "INSERT INTO email_requests (topic_id, email, subject, content, scheduled_at) VALUES (?, ?, ?, ?, datetime('now'))",
        )
        .bind("topic")
        .bind("test@example.com")
        .bind("subject")
        .bind("content")
        .execute(&db)
        .await
        .unwrap();

        let result1 = EmailResult {
            id: None,
            request_id: 1,
            status: "Delivery".into(),
            raw: None,
        }
        .save(&db)
        .await
        .unwrap();

        let result2 = EmailResult {
            id: None,
            request_id: 1,
            status: "Open".into(),
            raw: None,
        }
        .save(&db)
        .await
        .unwrap();

        assert_eq!(result1.id, Some(1));
        assert_eq!(result2.id, Some(2));
    }

    #[tokio::test]
    async fn test_result_with_raw_data() {
        let db = setup_db().await;

        sqlx::query(
            "INSERT INTO email_requests (topic_id, email, subject, content, scheduled_at) VALUES (?, ?, ?, ?, datetime('now'))",
        )
        .bind("topic")
        .bind("test@example.com")
        .bind("subject")
        .bind("content")
        .execute(&db)
        .await
        .unwrap();

        let raw_json = r#"{"timestamp":"2024-01-01","details":{"bounceType":"Permanent"}}"#;

        let saved = EmailResult {
            id: None,
            request_id: 1,
            status: "Bounce".into(),
            raw: Some(raw_json.to_string()),
        }
        .save(&db)
        .await
        .unwrap();

        let row: (Option<String>,) = sqlx::query_as("SELECT raw FROM email_results WHERE id = ?")
            .bind(saved.id)
            .fetch_one(&db)
            .await
            .unwrap();

        assert_eq!(row.0, Some(raw_json.to_string()));
    }

    #[tokio::test]
    async fn test_result_counts_different_topics() {
        let db = setup_db().await;

        // topic_a (id = 1)
        sqlx::query(
            "INSERT INTO email_requests (topic_id, email, subject, content, scheduled_at) VALUES (?, ?, ?, ?, datetime('now'))",
        )
        .bind("topic_a")
        .bind("a@example.com")
        .bind("subject")
        .bind("content")
        .execute(&db)
        .await
        .unwrap();

        // topic_b (id = 2)
        sqlx::query(
            "INSERT INTO email_requests (topic_id, email, subject, content, scheduled_at) VALUES (?, ?, ?, ?, datetime('now'))",
        )
        .bind("topic_b")
        .bind("b@example.com")
        .bind("subject")
        .bind("content")
        .execute(&db)
        .await
        .unwrap();

        EmailResult {
            id: None,
            request_id: 1,
            status: "Delivery".into(),
            raw: None,
        }
        .save(&db)
        .await
        .unwrap();

        EmailResult {
            id: None,
            request_id: 2,
            status: "Bounce".into(),
            raw: None,
        }
        .save(&db)
        .await
        .unwrap();

        let counts_a = EmailResult::get_result_counts_by_topic_id(&db, "topic_a")
            .await
            .unwrap();
        let counts_b = EmailResult::get_result_counts_by_topic_id(&db, "topic_b")
            .await
            .unwrap();

        assert_eq!(counts_a.get("Delivery"), Some(&1));
        assert!(counts_a.get("Bounce").is_none());

        assert_eq!(counts_b.get("Bounce"), Some(&1));
        assert!(counts_b.get("Delivery").is_none());
    }
}
