//! Scheduled email pickup service

use crate::models::request::{EmailMessageStatus, EmailRequest};
use sqlx::SqlitePool;
use std::time::Duration;
use thiserror::Error;
use tokio::sync::mpsc;
use tracing::{debug, error, info};

const BATCH_SIZE: i32 = 1000;
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

#[derive(sqlx::FromRow)]
struct ScheduledEmailRow {
    id: i64,
    topic_id: String,
    email: String,
    subject: String,
    content: String,
}

async fn fetch_and_process_batch(
    tx: &mpsc::Sender<EmailRequest>,
    db_pool: &SqlitePool,
) -> Result<usize, SchedulerError> {
    let rows: Vec<ScheduledEmailRow> = sqlx::query_as(
        "SELECT id, topic_id, email, subject, content
         FROM email_requests
         WHERE status = ? AND scheduled_at <= datetime('now')
         ORDER BY scheduled_at ASC
         LIMIT ?",
    )
    .bind(EmailMessageStatus::Created as i32)
    .bind(BATCH_SIZE)
    .fetch_all(db_pool)
    .await?;

    if rows.is_empty() {
        return Ok(0);
    }

    let mut sent_ids: Vec<i64> = Vec::with_capacity(rows.len());

    for row in rows {
        #[allow(clippy::cast_possible_truncation)]
        let request = EmailRequest {
            id: Some(row.id as i32),
            topic_id: Some(row.topic_id),
            email: row.email,
            subject: row.subject,
            content: row.content,
            scheduled_at: None,
            status: EmailMessageStatus::Processed as i32,
            error: None,
            message_id: None,
        };

        if tx.send(request).await.is_err() {
            if !sent_ids.is_empty() {
                update_status_batch(db_pool, &sent_ids).await?;
            }
            return Err(SchedulerError::ChannelClosed);
        }
        sent_ids.push(row.id);
    }

    let batch_size = sent_ids.len();
    if !sent_ids.is_empty() {
        update_status_batch(db_pool, &sent_ids).await?;
    }

    Ok(batch_size)
}

async fn update_status_batch(db_pool: &SqlitePool, ids: &[i64]) -> Result<(), SchedulerError> {
    let placeholders = vec!["?"; ids.len()];
    let sql = format!(
        "UPDATE email_requests SET status=?, updated_at=datetime('now') WHERE id IN ({})",
        placeholders.join(",")
    );

    let mut query = sqlx::query(&sql).bind(EmailMessageStatus::Processed as i32);
    for id in ids {
        query = query.bind(*id);
    }
    query.execute(db_pool).await?;
    Ok(())
}
