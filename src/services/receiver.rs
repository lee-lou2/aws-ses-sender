//! Rate-limited email sender and result processor

use std::{
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc,
    },
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use sqlx::SqlitePool;
use tokio::sync::{mpsc, Semaphore};
use tracing::{debug, error, info, warn};

use crate::{
    config,
    models::request::{EmailMessageStatus, EmailRequest},
};

// Token bucket configuration
const TOKEN_REFILL_INTERVAL_MS: u64 = 100;
const TOKEN_WAIT_INTERVAL_MS: u64 = 5;

// Batch update configuration
const BATCH_SIZE: usize = 100;
const BATCH_FLUSH_INTERVAL_MS: u64 = 500;
const BATCH_RECV_TIMEOUT_MS: u64 = 100;

/// Sends emails with rate limiting using Token Bucket + Semaphore.
pub async fn receive_send_message(
    mut rx: mpsc::Receiver<EmailRequest>,
    tx: mpsc::Sender<EmailRequest>,
) {
    let envs = config::get_environments();
    let max_per_sec = u64::try_from(envs.max_send_per_second.max(1)).unwrap_or(1);

    let tokens = Arc::new(AtomicU64::new(max_per_sec));
    let last_refill_ms = Arc::new(AtomicU64::new(current_time_ms()));
    let semaphore = Arc::new(Semaphore::new(
        usize::try_from(max_per_sec).unwrap_or(1) * 2,
    ));

    let server_url: Arc<str> = envs.server_url.clone().into();
    let from_email: Arc<str> = envs.aws_ses_from_email.clone().into();

    spawn_token_refill_task(
        Arc::clone(&tokens),
        Arc::clone(&last_refill_ms),
        max_per_sec,
    );

    info!("Email sender started: {max_per_sec} emails/sec");

    while let Some(mut request) = rx.recv().await {
        acquire_token(&tokens).await;

        let request_id = request.id.unwrap_or_default();
        request.content = format!(
            "{}<img src=\"{}/v1/events/open?request_id={}\">",
            request.content, server_url, request_id
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

fn spawn_token_refill_task(
    tokens: Arc<AtomicU64>,
    last_refill_ms: Arc<AtomicU64>,
    max_per_sec: u64,
) {
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_millis(TOKEN_REFILL_INTERVAL_MS));
        loop {
            interval.tick().await;

            let now_ms = current_time_ms();
            let last = last_refill_ms.load(Ordering::Acquire);

            if now_ms.saturating_sub(last) >= 1000 {
                tokens.store(max_per_sec, Ordering::Release);
                last_refill_ms.store(now_ms, Ordering::Release);
            } else {
                let refill = max_per_sec.div_ceil(10);
                let _ = tokens.fetch_update(Ordering::AcqRel, Ordering::Acquire, |current| {
                    if current < max_per_sec {
                        Some((current + refill).min(max_per_sec))
                    } else {
                        None
                    }
                });
            }
        }
    });
}

async fn acquire_token(tokens: &AtomicU64) {
    loop {
        let result = tokens.fetch_update(Ordering::AcqRel, Ordering::Acquire, |current| {
            if current > 0 {
                Some(current - 1)
            } else {
                None
            }
        });
        if result.is_ok() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(TOKEN_WAIT_INTERVAL_MS)).await;
    }
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

    // Update message_ids (individual, values differ)
    for (id, msg_id) in &message_id_updates {
        let _ = sqlx::query("UPDATE email_requests SET message_id=? WHERE id=?")
            .bind(msg_id)
            .bind(id)
            .execute(&mut *tx)
            .await;
    }

    // Update errors (individual, values differ)
    for (id, err) in &error_updates {
        let _ = sqlx::query("UPDATE email_requests SET error=? WHERE id=?")
            .bind(err)
            .bind(id)
            .execute(&mut *tx)
            .await;
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

async fn fallback_individual_updates(db_pool: &SqlitePool, batch: &mut Vec<EmailRequest>) {
    for req in batch.drain(..) {
        if let Err(e) = req.update(db_pool).await {
            error!("Update failed for id={:?}: {e:?}", req.id);
        }
    }
}
