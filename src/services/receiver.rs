//! Rate-limited email sender and result processor

use std::{
    fmt::Write as _,
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc,
    },
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use sqlx::SqlitePool;
use tokio::sync::{mpsc, Notify, Semaphore};
use tracing::{debug, error, info, warn};

use crate::{
    config,
    models::request::{EmailMessageStatus, EmailRequest},
};

// Token bucket configuration
const TOKEN_REFILL_INTERVAL_MS: u64 = 100;

// Batch update configuration
const BATCH_SIZE: usize = 100;
const BATCH_FLUSH_INTERVAL_MS: u64 = 500;
const BATCH_RECV_TIMEOUT_MS: u64 = 100;

/// Event-driven token bucket for rate limiting.
struct TokenBucket {
    tokens: AtomicU64,
    max_per_sec: u64,
    notify: Notify,
}

impl TokenBucket {
    fn new(max_per_sec: u64) -> Self {
        Self {
            tokens: AtomicU64::new(max_per_sec),
            max_per_sec,
            notify: Notify::new(),
        }
    }

    /// Acquires a token, waiting if necessary.
    async fn acquire(&self) {
        loop {
            if self.try_acquire() {
                return;
            }
            self.notify.notified().await;
        }
    }

    fn try_acquire(&self) -> bool {
        self.tokens
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |current| {
                if current > 0 {
                    Some(current - 1)
                } else {
                    None
                }
            })
            .is_ok()
    }

    fn refill(&self, amount: u64) {
        let _ = self
            .tokens
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |current| {
                if current < self.max_per_sec {
                    Some((current + amount).min(self.max_per_sec))
                } else {
                    None
                }
            });
        self.notify.notify_waiters();
    }

    fn reset(&self) {
        self.tokens.store(self.max_per_sec, Ordering::Release);
        self.notify.notify_waiters();
    }
}

/// Sends emails with rate limiting using Token Bucket + Semaphore.
pub async fn receive_send_message(
    mut rx: mpsc::Receiver<EmailRequest>,
    tx: mpsc::Sender<EmailRequest>,
) {
    let envs = config::get_environments();
    let max_per_sec = u64::try_from(envs.max_send_per_second.max(1)).unwrap_or(1);

    let bucket = Arc::new(TokenBucket::new(max_per_sec));
    let last_refill_ms = Arc::new(AtomicU64::new(current_time_ms()));
    let semaphore = Arc::new(Semaphore::new(
        usize::try_from(max_per_sec).unwrap_or(1) * 2,
    ));

    let server_url: Arc<str> = envs.server_url.clone().into();
    let from_email: Arc<str> = envs.aws_ses_from_email.clone().into();

    spawn_token_refill_task(Arc::clone(&bucket), Arc::clone(&last_refill_ms));

    info!("Email sender started: {max_per_sec} emails/sec");

    while let Some(mut request) = rx.recv().await {
        bucket.acquire().await;

        let request_id = request.id.unwrap_or_default();
        // Append tracking pixel
        let _ = write!(
            request.content,
            "<img src=\"{server_url}/v1/events/open?request_id={request_id}\">"
        );

        let tx_clone = tx.clone();
        let from_email = Arc::clone(&from_email);
        let Ok(permit) = semaphore.clone().acquire_owned().await else {
            break;
        };

        tokio::spawn(async move {
            let _permit = permit;

            match crate::services::sender::send_email(
                &from_email,
                &request.email,
                &request.subject,
                &request.content,
            )
            .await
            {
                Ok(message_id) => {
                    debug!("Sent to {}: {message_id}", request.email);
                    request.status = EmailMessageStatus::Sent as i32;
                    request.message_id = Some(message_id);
                }
                Err(e) => {
                    error!("Failed to send to {}: {e}", request.email);
                    request.status = EmailMessageStatus::Failed as i32;
                    request.error = Some(e.to_string());
                }
            }

            drop(tx_clone.send(request).await);
        });
    }

    warn!("Email sender stopped");
}

#[allow(clippy::cast_possible_truncation)]
fn current_time_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

fn spawn_token_refill_task(bucket: Arc<TokenBucket>, last_refill_ms: Arc<AtomicU64>) {
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_millis(TOKEN_REFILL_INTERVAL_MS));
        loop {
            interval.tick().await;

            let now_ms = current_time_ms();
            let last = last_refill_ms.load(Ordering::Acquire);

            if now_ms.saturating_sub(last) >= 1000 {
                bucket.reset();
                last_refill_ms.store(now_ms, Ordering::Release);
            } else {
                let refill = bucket.max_per_sec.div_ceil(10);
                bucket.refill(refill);
            }
        }
    });
}

/// Batches and persists email sending results.
pub async fn receive_post_send_message(mut rx: mpsc::Receiver<EmailRequest>, db_pool: SqlitePool) {
    let mut batch: Vec<EmailRequest> = Vec::with_capacity(BATCH_SIZE);
    let mut last_flush = Instant::now();
    let flush_interval = Duration::from_millis(BATCH_FLUSH_INTERVAL_MS);

    info!("Post-processor started: batch_size={BATCH_SIZE}");

    loop {
        match tokio::time::timeout(Duration::from_millis(BATCH_RECV_TIMEOUT_MS), rx.recv()).await {
            Ok(Some(request)) => {
                batch.push(request);
                if batch.len() >= BATCH_SIZE || last_flush.elapsed() >= flush_interval {
                    flush_batch(&db_pool, &mut batch).await;
                    last_flush = Instant::now();
                }
            }
            Ok(None) => {
                if !batch.is_empty() {
                    flush_batch(&db_pool, &mut batch).await;
                }
                info!("Post-processor stopped");
                break;
            }
            Err(_) => {
                if !batch.is_empty() && last_flush.elapsed() >= flush_interval {
                    flush_batch(&db_pool, &mut batch).await;
                    last_flush = Instant::now();
                }
            }
        }
    }
}

async fn flush_batch(db_pool: &SqlitePool, batch: &mut Vec<EmailRequest>) {
    if batch.is_empty() {
        return;
    }

    let count = batch.len();
    debug!("Flushing {count} results");

    // Group by status for bulk updates
    let mut sent_ids: Vec<i32> = Vec::new();
    let mut failed_ids: Vec<i32> = Vec::new();
    let mut message_id_updates: Vec<(i32, String)> = Vec::new();
    let mut error_updates: Vec<(i32, String)> = Vec::new();

    for req in batch.iter() {
        let id = req.id.unwrap_or_default();
        if req.status == EmailMessageStatus::Sent as i32 {
            sent_ids.push(id);
            if let Some(ref msg_id) = req.message_id {
                message_id_updates.push((id, msg_id.clone()));
            }
        } else if req.status == EmailMessageStatus::Failed as i32 {
            failed_ids.push(id);
            if let Some(ref err) = req.error {
                error_updates.push((id, err.clone()));
            }
        }
    }

    let Ok(mut tx) = db_pool.begin().await else {
        error!("Transaction begin failed");
        fallback_individual_updates(db_pool, batch).await;
        return;
    };

    // Bulk update sent status
    if !sent_ids.is_empty() {
        if let Err(e) = bulk_update_status(&mut tx, &sent_ids, EmailMessageStatus::Sent).await {
            error!("Bulk update sent failed: {e:?}");
        }
    }

    // Bulk update failed status
    if !failed_ids.is_empty() {
        if let Err(e) = bulk_update_status(&mut tx, &failed_ids, EmailMessageStatus::Failed).await {
            error!("Bulk update failed failed: {e:?}");
        }
    }

    // Bulk update message_ids using CASE WHEN
    if !message_id_updates.is_empty() {
        if let Err(e) = bulk_update_message_ids(&mut tx, &message_id_updates).await {
            error!("Bulk update message_ids failed: {e:?}");
        }
    }

    // Bulk update errors using CASE WHEN
    if !error_updates.is_empty() {
        if let Err(e) = bulk_update_errors(&mut tx, &error_updates).await {
            error!("Bulk update errors failed: {e:?}");
        }
    }

    if let Err(e) = tx.commit().await {
        error!("Transaction commit failed: {e:?}");
    }

    batch.clear();
}

async fn bulk_update_status(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    ids: &[i32],
    status: EmailMessageStatus,
) -> Result<(), sqlx::Error> {
    if ids.is_empty() {
        return Ok(());
    }

    let placeholders = vec!["?"; ids.len()].join(",");
    let sql = format!(
        "UPDATE email_requests SET status=?, updated_at=datetime('now') WHERE id IN ({placeholders})"
    );

    let mut query = sqlx::query(&sql).bind(status as i32);
    for id in ids {
        query = query.bind(id);
    }
    query.execute(&mut **tx).await?;
    Ok(())
}

/// Bulk update `message_id` values using CASE WHEN for better performance.
async fn bulk_update_message_ids(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    updates: &[(i32, String)],
) -> Result<(), sqlx::Error> {
    if updates.is_empty() {
        return Ok(());
    }

    // Build: UPDATE email_requests SET message_id = CASE id WHEN ? THEN ? ... END WHERE id IN (...)
    let case_parts = vec!["WHEN ? THEN ?"; updates.len()].join(" ");
    let placeholders = vec!["?"; updates.len()].join(",");
    let sql = format!(
        "UPDATE email_requests SET message_id = CASE id {case_parts} END WHERE id IN ({placeholders})"
    );

    let mut query = sqlx::query(&sql);
    for (id, msg_id) in updates {
        query = query.bind(id).bind(msg_id);
    }
    for (id, _) in updates {
        query = query.bind(id);
    }
    query.execute(&mut **tx).await?;
    Ok(())
}

/// Bulk update errors using CASE WHEN for better performance.
async fn bulk_update_errors(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    updates: &[(i32, String)],
) -> Result<(), sqlx::Error> {
    if updates.is_empty() {
        return Ok(());
    }

    let case_parts = vec!["WHEN ? THEN ?"; updates.len()].join(" ");
    let placeholders = vec!["?"; updates.len()].join(",");
    let sql = format!(
        "UPDATE email_requests SET error = CASE id {case_parts} END WHERE id IN ({placeholders})"
    );

    let mut query = sqlx::query(&sql);
    for (id, err) in updates {
        query = query.bind(id).bind(err);
    }
    for (id, _) in updates {
        query = query.bind(id);
    }
    query.execute(&mut **tx).await?;
    Ok(())
}

async fn fallback_individual_updates(db_pool: &SqlitePool, batch: &mut Vec<EmailRequest>) {
    for req in batch.drain(..) {
        if let Err(e) = req.update(db_pool).await {
            error!("Update failed for id={:?}: {e:?}", req.id);
        }
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
                FOREIGN KEY (content_id) REFERENCES email_contents(id)
            )",
        )
        .execute(&pool)
        .await
        .unwrap();

        pool
    }

    async fn insert_test_content(pool: &SqlitePool) -> i64 {
        let row: (i64,) = sqlx::query_as(
            "INSERT INTO email_contents (subject, content) VALUES ('Test', 'Test') RETURNING id",
        )
        .fetch_one(pool)
        .await
        .unwrap();
        row.0
    }

    async fn insert_test_request(pool: &SqlitePool, content_id: i64, id: i32) {
        sqlx::query(
            "INSERT INTO email_requests (id, topic_id, content_id, email, scheduled_at, status)
             VALUES (?, 'test', ?, 'test@test.com', datetime('now'), 0)",
        )
        .bind(id)
        .bind(content_id)
        .execute(pool)
        .await
        .unwrap();
    }

    #[test]
    fn test_token_bucket_new() {
        let bucket = TokenBucket::new(10);
        assert_eq!(bucket.tokens.load(Ordering::Acquire), 10);
        assert_eq!(bucket.max_per_sec, 10);
    }

    #[test]
    fn test_token_bucket_try_acquire() {
        let bucket = TokenBucket::new(2);

        assert!(bucket.try_acquire());
        assert_eq!(bucket.tokens.load(Ordering::Acquire), 1);

        assert!(bucket.try_acquire());
        assert_eq!(bucket.tokens.load(Ordering::Acquire), 0);

        assert!(!bucket.try_acquire());
        assert_eq!(bucket.tokens.load(Ordering::Acquire), 0);
    }

    #[test]
    fn test_token_bucket_refill() {
        let bucket = TokenBucket::new(10);

        // Consume all tokens
        for _ in 0..10 {
            bucket.try_acquire();
        }
        assert_eq!(bucket.tokens.load(Ordering::Acquire), 0);

        // Refill 3
        bucket.refill(3);
        assert_eq!(bucket.tokens.load(Ordering::Acquire), 3);

        // Refill beyond max
        bucket.refill(20);
        assert_eq!(bucket.tokens.load(Ordering::Acquire), 10);
    }

    #[test]
    fn test_token_bucket_reset() {
        let bucket = TokenBucket::new(10);

        // Consume all
        for _ in 0..10 {
            bucket.try_acquire();
        }
        assert_eq!(bucket.tokens.load(Ordering::Acquire), 0);

        bucket.reset();
        assert_eq!(bucket.tokens.load(Ordering::Acquire), 10);
    }

    #[tokio::test]
    async fn test_token_bucket_acquire() {
        let bucket = Arc::new(TokenBucket::new(2));

        bucket.acquire().await;
        assert_eq!(bucket.tokens.load(Ordering::Acquire), 1);

        bucket.acquire().await;
        assert_eq!(bucket.tokens.load(Ordering::Acquire), 0);
    }

    #[tokio::test]
    async fn test_token_bucket_acquire_waits_for_refill() {
        let bucket = Arc::new(TokenBucket::new(1));

        bucket.acquire().await;
        assert_eq!(bucket.tokens.load(Ordering::Acquire), 0);

        let bucket_clone = Arc::clone(&bucket);
        let handle = tokio::spawn(async move {
            bucket_clone.acquire().await;
        });

        // Allow spawn to start waiting
        tokio::time::sleep(Duration::from_millis(10)).await;

        // Refill should wake up the waiter
        bucket.refill(1);

        // Should complete within reasonable time
        tokio::time::timeout(Duration::from_millis(100), handle)
            .await
            .expect("acquire should complete after refill")
            .unwrap();
    }

    #[tokio::test]
    async fn test_bulk_update_message_ids() {
        let db = setup_db().await;
        let content_id = insert_test_content(&db).await;

        insert_test_request(&db, content_id, 1).await;
        insert_test_request(&db, content_id, 2).await;
        insert_test_request(&db, content_id, 3).await;

        let updates = vec![
            (1, "msg_id_1".to_string()),
            (2, "msg_id_2".to_string()),
            (3, "msg_id_3".to_string()),
        ];

        let mut tx = db.begin().await.unwrap();
        bulk_update_message_ids(&mut tx, &updates).await.unwrap();
        tx.commit().await.unwrap();

        let rows: Vec<(i32, Option<String>)> =
            sqlx::query_as("SELECT id, message_id FROM email_requests ORDER BY id")
                .fetch_all(&db)
                .await
                .unwrap();

        assert_eq!(rows[0].1, Some("msg_id_1".to_string()));
        assert_eq!(rows[1].1, Some("msg_id_2".to_string()));
        assert_eq!(rows[2].1, Some("msg_id_3".to_string()));
    }

    #[tokio::test]
    async fn test_bulk_update_errors() {
        let db = setup_db().await;
        let content_id = insert_test_content(&db).await;

        insert_test_request(&db, content_id, 1).await;
        insert_test_request(&db, content_id, 2).await;

        let updates = vec![(1, "Error 1".to_string()), (2, "Error 2".to_string())];

        let mut tx = db.begin().await.unwrap();
        bulk_update_errors(&mut tx, &updates).await.unwrap();
        tx.commit().await.unwrap();

        let rows: Vec<(i32, Option<String>)> =
            sqlx::query_as("SELECT id, error FROM email_requests ORDER BY id")
                .fetch_all(&db)
                .await
                .unwrap();

        assert_eq!(rows[0].1, Some("Error 1".to_string()));
        assert_eq!(rows[1].1, Some("Error 2".to_string()));
    }

    #[tokio::test]
    async fn test_bulk_update_status() {
        let db = setup_db().await;
        let content_id = insert_test_content(&db).await;

        insert_test_request(&db, content_id, 1).await;
        insert_test_request(&db, content_id, 2).await;
        insert_test_request(&db, content_id, 3).await;

        let mut tx = db.begin().await.unwrap();
        bulk_update_status(&mut tx, &[1, 2], EmailMessageStatus::Sent)
            .await
            .unwrap();
        tx.commit().await.unwrap();

        let rows: Vec<(i32, i32)> =
            sqlx::query_as("SELECT id, status FROM email_requests ORDER BY id")
                .fetch_all(&db)
                .await
                .unwrap();

        assert_eq!(rows[0].1, EmailMessageStatus::Sent as i32);
        assert_eq!(rows[1].1, EmailMessageStatus::Sent as i32);
        assert_eq!(rows[2].1, 0); // Unchanged
    }

    #[tokio::test]
    async fn test_bulk_update_empty() {
        let db = setup_db().await;

        let mut tx = db.begin().await.unwrap();
        bulk_update_status(&mut tx, &[], EmailMessageStatus::Sent)
            .await
            .unwrap();
        bulk_update_message_ids(&mut tx, &[]).await.unwrap();
        bulk_update_errors(&mut tx, &[]).await.unwrap();
        tx.commit().await.unwrap();
    }
}
