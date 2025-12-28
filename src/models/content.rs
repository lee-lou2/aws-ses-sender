//! Email content model for deduplication

use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;
use tracing::debug;

use crate::constants::BATCH_INSERT_SIZE;

/// Email content entity (subject + body)
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct EmailContent {
    pub id: Option<i32>,
    pub subject: String,
    pub content: String,
}

impl EmailContent {
    /// Saves email content and returns the saved entity with ID.
    #[cfg(test)]
    pub async fn save(self, db_pool: &SqlitePool) -> Result<Self, sqlx::Error> {
        let row: (i64,) = sqlx::query_as(
            "INSERT INTO email_contents (subject, content, created_at)
             VALUES (?, ?, datetime('now'))
             RETURNING id",
        )
        .bind(&self.subject)
        .bind(&self.content)
        .fetch_one(db_pool)
        .await?;

        #[allow(clippy::cast_possible_truncation)]
        Ok(Self {
            id: Some(row.0 as i32),
            ..self
        })
    }

    /// Saves multiple contents using multi-row INSERT for better performance.
    ///
    /// This provides ~2-5x performance improvement over individual inserts.
    pub async fn save_batch(
        contents: Vec<Self>,
        db_pool: &SqlitePool,
    ) -> Result<Vec<Self>, sqlx::Error> {
        if contents.is_empty() {
            return Ok(Vec::new());
        }

        let total = contents.len();
        let mut results = Vec::with_capacity(total);
        let mut tx = db_pool.begin().await?;

        for chunk in contents.chunks(BATCH_INSERT_SIZE) {
            let chunk_size = chunk.len();

            let placeholders = (0..chunk_size)
                .map(|_| "(?, ?, datetime('now'))")
                .collect::<Vec<_>>()
                .join(", ");

            let sql = format!(
                "INSERT INTO email_contents (subject, content, created_at) VALUES {placeholders}"
            );

            let mut query = sqlx::query(&sql);
            for c in chunk {
                query = query.bind(&c.subject).bind(&c.content);
            }
            query.execute(&mut *tx).await?;

            let (last_id,): (i64,) = sqlx::query_as("SELECT last_insert_rowid()")
                .fetch_one(&mut *tx)
                .await?;

            #[allow(clippy::cast_possible_truncation, clippy::cast_possible_wrap)]
            for (i, c) in chunk.iter().enumerate() {
                let id = (last_id - (chunk_size - 1 - i) as i64) as i32;
                results.push(Self {
                    id: Some(id),
                    ..c.clone()
                });
            }
        }

        tx.commit().await?;
        debug!("Batch inserted {total} contents");

        Ok(results)
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

        pool
    }

    #[tokio::test]
    async fn test_save_returns_id() {
        let db = setup_db().await;

        let content = EmailContent {
            id: None,
            subject: "Test Subject".to_string(),
            content: "<p>Test Content</p>".to_string(),
        };

        let saved = content.save(&db).await.unwrap();
        assert_eq!(saved.id, Some(1));
        assert_eq!(saved.subject, "Test Subject");
    }

    #[tokio::test]
    async fn test_save_batch() {
        let db = setup_db().await;

        let contents = vec![
            EmailContent {
                id: None,
                subject: "Subject 1".to_string(),
                content: "Content 1".to_string(),
            },
            EmailContent {
                id: None,
                subject: "Subject 2".to_string(),
                content: "Content 2".to_string(),
            },
        ];

        let saved = EmailContent::save_batch(contents, &db).await.unwrap();
        assert_eq!(saved.len(), 2);
        assert_eq!(saved[0].id, Some(1));
        assert_eq!(saved[1].id, Some(2));
    }

    #[tokio::test]
    async fn test_save_batch_large() {
        let db = setup_db().await;

        let contents: Vec<EmailContent> = (0..150)
            .map(|i| EmailContent {
                id: None,
                subject: format!("Subject {i}"),
                content: format!("Content {i}"),
            })
            .collect();

        let saved = EmailContent::save_batch(contents, &db).await.unwrap();
        assert_eq!(saved.len(), 150);
        assert_eq!(saved[0].id, Some(1));
        assert_eq!(saved[149].id, Some(150));
    }

    #[tokio::test]
    async fn test_save_batch_empty() {
        let db = setup_db().await;
        let contents: Vec<EmailContent> = vec![];
        let saved = EmailContent::save_batch(contents, &db).await.unwrap();
        assert!(saved.is_empty());
    }

    #[tokio::test]
    async fn test_save_batch_single() {
        let db = setup_db().await;

        let contents = vec![EmailContent {
            id: None,
            subject: "Single Subject".to_string(),
            content: "Single Content".to_string(),
        }];

        let saved = EmailContent::save_batch(contents, &db).await.unwrap();
        assert_eq!(saved.len(), 1);
        assert_eq!(saved[0].id, Some(1));
        assert_eq!(saved[0].subject, "Single Subject");
    }

    #[tokio::test]
    async fn test_save_preserves_special_characters() {
        let db = setup_db().await;

        let content = EmailContent {
            id: None,
            subject: "Hello 'World' \"Test\" <>&".to_string(),
            content: "<p>HTML content with émojis 🎉 and 한글</p>".to_string(),
        };

        let saved = content.save(&db).await.unwrap();
        assert_eq!(saved.subject, "Hello 'World' \"Test\" <>&");
        assert_eq!(saved.content, "<p>HTML content with émojis 🎉 and 한글</p>");
    }

    #[tokio::test]
    async fn test_save_batch_preserves_order() {
        let db = setup_db().await;

        let contents: Vec<EmailContent> = (0..10)
            .map(|i| EmailContent {
                id: None,
                subject: format!("Subject {i}"),
                content: format!("Content {i}"),
            })
            .collect();

        let saved = EmailContent::save_batch(contents, &db).await.unwrap();

        for (i, c) in saved.iter().enumerate() {
            assert_eq!(c.subject, format!("Subject {i}"));
            assert_eq!(c.content, format!("Content {i}"));
        }
    }

    #[tokio::test]
    async fn test_save_long_content() {
        let db = setup_db().await;

        let long_content = "x".repeat(100_000);
        let content = EmailContent {
            id: None,
            subject: "Long Content Test".to_string(),
            content: long_content.clone(),
        };

        let saved = content.save(&db).await.unwrap();
        assert_eq!(saved.id, Some(1));

        let row: (String,) = sqlx::query_as("SELECT content FROM email_contents WHERE id = 1")
            .fetch_one(&db)
            .await
            .unwrap();
        assert_eq!(row.0.len(), 100_000);
    }
}
