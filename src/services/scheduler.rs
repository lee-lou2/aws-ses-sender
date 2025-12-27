//! Scheduled email pickup service

use std::time::Duration;

use sqlx::SqlitePool;
use thiserror::Error;
use tokio::sync::mpsc;
use tracing::{debug, error, info};

use crate::models::request::{EmailMessageStatus, EmailRequest};

const BATCH_SIZE: i32 = 1000;

// Polling interval configuration
const IDLE_DELAY_SECS: u64 = 10;
const BATCH_DELAY_MS: u64 = 100;
const ERROR_BACKOFF_SECS: u64 = 5;

/// Polls for scheduled emails and forwards them to the sending queue.
pub async fn schedule_pre_send_message(tx: &mpsc::Sender<EmailRequest>, db_pool: SqlitePool) {
    info!("Scheduler started: batch_size={BATCH_SIZE}");

    let mut consecutive_empty = 0u32;

    loop {
        match fetch_and_process_batch(tx, &db_pool).await {
            Ok(0) => {
                consecutive_empty += 1;
                let delay = if consecutive_empty > 5 {
                    IDLE_DELAY_SECS * 2
                } else {
                    IDLE_DELAY_SECS
                };
                debug!("No messages, sleeping {delay}s");
                tokio::time::sleep(Duration::from_secs(delay)).await;
            }
            Ok(count) => {
                consecutive_empty = 0;
                debug!("Processed {count} messages");
                tokio::time::sleep(Duration::from_millis(BATCH_DELAY_MS)).await;
            }
            Err(e) => {
                error!("Scheduler error: {e}");
                tokio::time::sleep(Duration::from_secs(ERROR_BACKOFF_SECS)).await;
            }
        }
    }
}

#[derive(Debug, Error)]
enum SchedulerError {
    #[error("Database error: {0}")]
    Database(#[from] sqlx::Error),

    #[error("Send channel closed")]
    ChannelClosed,
}

/// Row returned from UPDATE...RETURNING (without content fields).
#[derive(sqlx::FromRow)]
struct UpdatedRow {
    id: i64,
    #[allow(dead_code)]
    topic_id: String,
    #[allow(dead_code)]
    content_id: i32,
    #[allow(dead_code)]
    email: String,
}

/// Row with content joined from `email_contents`.
#[derive(sqlx::FromRow)]
struct ScheduledEmailRow {
    id: i64,
    topic_id: String,
    content_id: i32,
    email: String,
    subject: String,
    content: String,
}

/// Atomically claims and processes a batch of scheduled emails.
///
/// Uses two-phase approach to avoid per-row subqueries in RETURNING:
/// 1. UPDATE...RETURNING to atomically claim emails and get basic info
/// 2. Single JOIN query to fetch content for all claimed emails
async fn fetch_and_process_batch(
    tx: &mpsc::Sender<EmailRequest>,
    db_pool: &SqlitePool,
) -> Result<usize, SchedulerError> {
    // Phase 1: Atomically update and return basic info (no subqueries)
    let updated: Vec<UpdatedRow> = sqlx::query_as(
        "UPDATE email_requests
         SET status = ?, updated_at = datetime('now')
         WHERE id IN (
             SELECT id FROM email_requests
             WHERE status = ? AND scheduled_at <= datetime('now')
             ORDER BY scheduled_at ASC
             LIMIT ?
         )
         RETURNING id, topic_id, content_id, email",
    )
    .bind(EmailMessageStatus::Processed as i32)
    .bind(EmailMessageStatus::Created as i32)
    .bind(BATCH_SIZE)
    .fetch_all(db_pool)
    .await?;

    if updated.is_empty() {
        return Ok(0);
    }

    let count = updated.len();

    // Phase 2: Fetch content with single JOIN query
    let ids: Vec<i64> = updated.iter().map(|r| r.id).collect();
    let placeholders = vec!["?"; ids.len()].join(",");
    let sql = format!(
        "SELECT r.id, r.topic_id, r.content_id, r.email, c.subject, c.content
         FROM email_requests r
         JOIN email_contents c ON r.content_id = c.id
         WHERE r.id IN ({placeholders})"
    );

    let mut query = sqlx::query_as::<_, ScheduledEmailRow>(&sql);
    for id in &ids {
        query = query.bind(id);
    }
    let rows = query.fetch_all(db_pool).await?;

    for row in rows {
        #[allow(clippy::cast_possible_truncation)]
        let request = EmailRequest {
            id: Some(row.id as i32),
            topic_id: Some(row.topic_id),
            content_id: Some(row.content_id),
            email: row.email,
            subject: row.subject,
            content: row.content,
            scheduled_at: None,
            status: EmailMessageStatus::Processed as i32,
            error: None,
            message_id: None,
        };

        if tx.send(request).await.is_err() {
            return Err(SchedulerError::ChannelClosed);
        }
    }

    Ok(count)
}
