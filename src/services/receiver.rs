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

    while let Some(request) = rx.recv().await {
        bucket.acquire().await;

        let request_id = request.id.unwrap_or_default();
        // Clone content from Arc and append tracking pixel
        // This defers the clone to send time (vs creation time for all emails)
        let mut content = (*request.content).clone();
        let _ = write!(
            content,
            "<img src=\"{server_url}/v1/events/open?request_id={request_id}\">"
        );

        let tx_clone = tx.clone();
        let from_email = Arc::clone(&from_email);
        let subject = Arc::clone(&request.subject);
        let email = request.email.clone();
        let Ok(permit) = semaphore.clone().acquire_owned().await else {
            break;
        };

        // Move request ownership into spawned task for result handling
        let mut request = request;
        tokio::spawn(async move {
            let _permit = permit;

            match crate::services::sender::send_email(&from_email, &email, &subject, &content).await
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

    if let Err(e) = bulk_update_all(db_pool, batch).await {
        error!("Bulk update failed: {e:?}, falling back to individual updates");
        fallback_individual_updates(db_pool, batch).await;
        return;
    }

    batch.clear();
}

/// Unified bulk update for status, `message_id`, and error fields.
async fn bulk_update_all(db_pool: &SqlitePool, batch: &[EmailRequest]) -> Result<(), sqlx::Error> {
    if batch.is_empty() {
        return Ok(());
    }

    let ids: Vec<i32> = batch.iter().filter_map(|r| r.id).collect();
    if ids.is_empty() {
        return Ok(());
    }

    // Build CASE WHEN clauses - separate binds to maintain correct order
    // Pre-allocate capacity to avoid reallocations during iteration
    let batch_len = batch.len();
    let mut status_cases = String::with_capacity(batch_len * 20);
    let mut message_id_cases = String::with_capacity(batch_len * 20);
    let mut error_cases = String::with_capacity(batch_len * 20);
    let mut status_binds: Vec<i32> = Vec::with_capacity(batch_len);
    let mut message_id_binds: Vec<String> = Vec::with_capacity(batch_len);
    let mut error_binds: Vec<String> = Vec::with_capacity(batch_len);

    for req in batch {
        let Some(id) = req.id else { continue };

        // Status case
        let _ = write!(status_cases, "WHEN {id} THEN ? ");
        status_binds.push(req.status);

        // Message ID case (only if present)
        if let Some(ref msg_id) = req.message_id {
            let _ = write!(message_id_cases, "WHEN {id} THEN ? ");
            message_id_binds.push(msg_id.clone());
        }

        // Error case (only if present)
        if let Some(ref err) = req.error {
            let _ = write!(error_cases, "WHEN {id} THEN ? ");
            error_binds.push(err.clone());
        }
    }

    let placeholders = ids.iter().map(|_| "?").collect::<Vec<_>>().join(",");

    // Build SQL with conditional CASE WHEN clauses
    let message_id_sql = if message_id_cases.is_empty() {
        String::new()
    } else {
        format!(", message_id = CASE id {message_id_cases}ELSE message_id END")
    };

    let error_sql = if error_cases.is_empty() {
        String::new()
    } else {
        format!(", error = CASE id {error_cases}ELSE error END")
    };

    let sql = format!(
        "UPDATE email_requests SET status = CASE id {status_cases}ELSE status END{message_id_sql}{error_sql}, updated_at = datetime('now') WHERE id IN ({placeholders})"
    );

    let mut query = sqlx::query(&sql);

    // Bind values in order: status cases, message_id cases, error cases, then ids
    for status in &status_binds {
        query = query.bind(*status);
    }
    for msg_id in &message_id_binds {
        query = query.bind(msg_id);
    }
    for err in &error_binds {
        query = query.bind(err);
    }
    for id in &ids {
        query = query.bind(*id);
    }

    query.execute(db_pool).await?;
    Ok(())
}

/// Bulk update status (kept for testing individual components).
#[allow(dead_code)]
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

/// Bulk update `message_id` values (kept for testing individual components).
#[allow(dead_code)]
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

/// Bulk update errors (kept for testing individual components).
#[allow(dead_code)]
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

    #[tokio::test]
    async fn test_bulk_update_all_unified() {
        let db = setup_db().await;
        let content_id = insert_test_content(&db).await;

        insert_test_request(&db, content_id, 1).await;
        insert_test_request(&db, content_id, 2).await;
        insert_test_request(&db, content_id, 3).await;

        // Create batch with mixed statuses
        let batch = vec![
            EmailRequest {
                id: Some(1),
                topic_id: Some("test".to_string()),
                content_id: Some(content_id as i32),
                email: "test1@test.com".to_string(),
                subject: Arc::new(String::new()),
                content: Arc::new(String::new()),
                scheduled_at: None,
                status: EmailMessageStatus::Sent as i32,
                message_id: Some("msg_1".to_string()),
                error: None,
            },
            EmailRequest {
                id: Some(2),
                topic_id: Some("test".to_string()),
                content_id: Some(content_id as i32),
                email: "test2@test.com".to_string(),
                subject: Arc::new(String::new()),
                content: Arc::new(String::new()),
                scheduled_at: None,
                status: EmailMessageStatus::Failed as i32,
                message_id: None,
                error: Some("Rate limit".to_string()),
            },
            EmailRequest {
                id: Some(3),
                topic_id: Some("test".to_string()),
                content_id: Some(content_id as i32),
                email: "test3@test.com".to_string(),
                subject: Arc::new(String::new()),
                content: Arc::new(String::new()),
                scheduled_at: None,
                status: EmailMessageStatus::Sent as i32,
                message_id: Some("msg_3".to_string()),
                error: None,
            },
        ];

        bulk_update_all(&db, &batch).await.unwrap();

        // Verify results using Row to handle SQLite type variations
        use sqlx::Row;
        let rows =
            sqlx::query("SELECT id, status, message_id, error FROM email_requests ORDER BY id")
                .fetch_all(&db)
                .await
                .unwrap();

        // ID 1: Sent with message_id
        let status1: i32 = rows[0].get("status");
        assert_eq!(status1, EmailMessageStatus::Sent as i32);
        assert_eq!(
            rows[0].get::<Option<String>, _>("message_id"),
            Some("msg_1".to_string())
        );
        assert_eq!(rows[0].get::<Option<String>, _>("error"), None);

        // ID 2: Failed with error
        let status2: i32 = rows[1].get("status");
        assert_eq!(status2, EmailMessageStatus::Failed as i32);
        assert_eq!(rows[1].get::<Option<String>, _>("message_id"), None);
        assert_eq!(
            rows[1].get::<Option<String>, _>("error"),
            Some("Rate limit".to_string())
        );

        // ID 3: Sent with message_id
        let status3: i32 = rows[2].get("status");
        assert_eq!(status3, EmailMessageStatus::Sent as i32);
        assert_eq!(
            rows[2].get::<Option<String>, _>("message_id"),
            Some("msg_3".to_string())
        );
        assert_eq!(rows[2].get::<Option<String>, _>("error"), None);
    }

    #[tokio::test]
    async fn test_bulk_update_all_empty_batch() {
        let db = setup_db().await;
        let result = bulk_update_all(&db, &[]).await;
        assert!(result.is_ok());
    }

    // === Concurrency tests for Token Bucket ===

    #[tokio::test]
    async fn test_token_bucket_concurrent_acquire() {
        use std::sync::atomic::AtomicUsize;

        let bucket = Arc::new(TokenBucket::new(5));
        let acquired_count = Arc::new(AtomicUsize::new(0));

        // Spawn 10 tasks trying to acquire tokens
        let mut handles = vec![];
        for _ in 0..10 {
            let bucket_clone = Arc::clone(&bucket);
            let count_clone = Arc::clone(&acquired_count);
            handles.push(tokio::spawn(async move {
                if bucket_clone.try_acquire() {
                    count_clone.fetch_add(1, Ordering::SeqCst);
                }
            }));
        }

        for handle in handles {
            handle.await.unwrap();
        }

        // Only 5 should have succeeded
        assert_eq!(acquired_count.load(Ordering::SeqCst), 5);
        assert_eq!(bucket.tokens.load(Ordering::Acquire), 0);
    }

    #[tokio::test]
    async fn test_token_bucket_concurrent_acquire_with_refill() {
        let bucket = Arc::new(TokenBucket::new(2));
        let acquired_count = Arc::new(AtomicU64::new(0));

        // Spawn 6 tasks trying to acquire (2 immediately, 4 should wait for refill)
        let mut handles = vec![];
        for _ in 0..6 {
            let bucket_clone = Arc::clone(&bucket);
            let count_clone = Arc::clone(&acquired_count);
            handles.push(tokio::spawn(async move {
                bucket_clone.acquire().await;
                count_clone.fetch_add(1, Ordering::SeqCst);
            }));
        }

        // Give time for initial acquires
        tokio::time::sleep(Duration::from_millis(10)).await;

        // Should have acquired 2 immediately
        assert!(acquired_count.load(Ordering::SeqCst) >= 2);

        // Refill to allow more
        bucket.refill(2);
        tokio::time::sleep(Duration::from_millis(10)).await;

        bucket.refill(2);
        tokio::time::sleep(Duration::from_millis(10)).await;

        // All should complete eventually
        for handle in handles {
            tokio::time::timeout(Duration::from_millis(100), handle)
                .await
                .expect("all acquires should complete")
                .unwrap();
        }

        assert_eq!(acquired_count.load(Ordering::SeqCst), 6);
    }

    #[tokio::test]
    async fn test_token_bucket_reset_wakes_waiters() {
        let bucket = Arc::new(TokenBucket::new(1));

        // Consume the only token
        bucket.acquire().await;

        let bucket_clone = Arc::clone(&bucket);
        let handle = tokio::spawn(async move {
            bucket_clone.acquire().await;
        });

        // Allow spawn to start waiting
        tokio::time::sleep(Duration::from_millis(10)).await;

        // Reset should wake up the waiter
        bucket.reset();

        // Should complete within reasonable time
        tokio::time::timeout(Duration::from_millis(100), handle)
            .await
            .expect("acquire should complete after reset")
            .unwrap();
    }

    #[tokio::test]
    async fn test_token_bucket_multiple_refills() {
        let bucket = TokenBucket::new(10);

        // Consume all tokens
        for _ in 0..10 {
            assert!(bucket.try_acquire());
        }
        assert_eq!(bucket.tokens.load(Ordering::Acquire), 0);

        // Multiple small refills
        bucket.refill(3);
        assert_eq!(bucket.tokens.load(Ordering::Acquire), 3);

        bucket.refill(3);
        assert_eq!(bucket.tokens.load(Ordering::Acquire), 6);

        bucket.refill(3);
        assert_eq!(bucket.tokens.load(Ordering::Acquire), 9);

        bucket.refill(3);
        assert_eq!(bucket.tokens.load(Ordering::Acquire), 10); // Capped at max
    }

    #[tokio::test]
    async fn test_token_bucket_stress_acquire_refill() {
        let bucket = Arc::new(TokenBucket::new(10));
        let total_acquired = Arc::new(AtomicU64::new(0));

        // Spawn multiple acquire tasks
        let mut acquire_handles = vec![];
        for _ in 0..20 {
            let bucket_clone = Arc::clone(&bucket);
            let count_clone = Arc::clone(&total_acquired);
            acquire_handles.push(tokio::spawn(async move {
                bucket_clone.acquire().await;
                count_clone.fetch_add(1, Ordering::SeqCst);
            }));
        }

        // Refill periodically
        let bucket_refill = Arc::clone(&bucket);
        let refill_handle = tokio::spawn(async move {
            for _ in 0..5 {
                tokio::time::sleep(Duration::from_millis(10)).await;
                bucket_refill.refill(5);
            }
        });

        refill_handle.await.unwrap();

        // Wait for all acquires with timeout
        for handle in acquire_handles {
            tokio::time::timeout(Duration::from_millis(500), handle)
                .await
                .expect("acquire should complete")
                .unwrap();
        }

        assert_eq!(total_acquired.load(Ordering::SeqCst), 20);
    }

    #[test]
    fn test_token_bucket_try_acquire_atomic() {
        let bucket = TokenBucket::new(1);

        // First try should succeed
        assert!(bucket.try_acquire());
        assert_eq!(bucket.tokens.load(Ordering::Acquire), 0);

        // Second try should fail
        assert!(!bucket.try_acquire());
        assert_eq!(bucket.tokens.load(Ordering::Acquire), 0);
    }

    #[tokio::test]
    async fn test_bulk_update_large_batch() {
        let db = setup_db().await;
        let content_id = insert_test_content(&db).await;

        // Insert 100 requests
        for i in 1..=100 {
            insert_test_request(&db, content_id, i).await;
        }

        // Create batch with all Sent status
        let batch: Vec<EmailRequest> = (1..=100)
            .map(|i| EmailRequest {
                id: Some(i),
                topic_id: Some("bulk_test".to_string()),
                content_id: Some(content_id as i32),
                email: format!("test{i}@test.com"),
                subject: Arc::new(String::new()),
                content: Arc::new(String::new()),
                scheduled_at: None,
                status: EmailMessageStatus::Sent as i32,
                message_id: Some(format!("msg_{i}")),
                error: None,
            })
            .collect();

        bulk_update_all(&db, &batch).await.unwrap();

        // Verify all updated
        let count: (i32,) = sqlx::query_as(
            "SELECT COUNT(*) FROM email_requests WHERE status = ? AND message_id IS NOT NULL",
        )
        .bind(EmailMessageStatus::Sent as i32)
        .fetch_one(&db)
        .await
        .unwrap();

        assert_eq!(count.0, 100);
    }

    #[tokio::test]
    async fn test_bulk_update_mixed_statuses_large() {
        let db = setup_db().await;
        let content_id = insert_test_content(&db).await;

        // Insert 50 requests
        for i in 1..=50 {
            insert_test_request(&db, content_id, i).await;
        }

        // Create batch with alternating Sent/Failed statuses
        let batch: Vec<EmailRequest> = (1..=50)
            .map(|i| {
                if i % 2 == 0 {
                    EmailRequest {
                        id: Some(i),
                        topic_id: Some("mixed_test".to_string()),
                        content_id: Some(content_id as i32),
                        email: format!("test{i}@test.com"),
                        subject: Arc::new(String::new()),
                        content: Arc::new(String::new()),
                        scheduled_at: None,
                        status: EmailMessageStatus::Sent as i32,
                        message_id: Some(format!("msg_{i}")),
                        error: None,
                    }
                } else {
                    EmailRequest {
                        id: Some(i),
                        topic_id: Some("mixed_test".to_string()),
                        content_id: Some(content_id as i32),
                        email: format!("test{i}@test.com"),
                        subject: Arc::new(String::new()),
                        content: Arc::new(String::new()),
                        scheduled_at: None,
                        status: EmailMessageStatus::Failed as i32,
                        message_id: None,
                        error: Some(format!("Error for {i}")),
                    }
                }
            })
            .collect();

        bulk_update_all(&db, &batch).await.unwrap();

        // Verify counts
        let sent_count: (i32,) =
            sqlx::query_as("SELECT COUNT(*) FROM email_requests WHERE status = ?")
                .bind(EmailMessageStatus::Sent as i32)
                .fetch_one(&db)
                .await
                .unwrap();

        let failed_count: (i32,) =
            sqlx::query_as("SELECT COUNT(*) FROM email_requests WHERE status = ?")
                .bind(EmailMessageStatus::Failed as i32)
                .fetch_one(&db)
                .await
                .unwrap();

        assert_eq!(sent_count.0, 25);
        assert_eq!(failed_count.0, 25);
    }
}
