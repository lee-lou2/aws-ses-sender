//! Rate-limited email sender and result processor

use crate::{
    config,
    models::request::{EmailMessageStatus, EmailRequest},
};
use sqlx::SqlitePool;
use std::{
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc,
    },
    time::{Duration, Instant},
};
use tokio::sync::{mpsc, Semaphore};
use tracing::{debug, error, info, warn};

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
    let last_refill = Arc::new(std::sync::Mutex::new(Instant::now()));
    let semaphore = Arc::new(Semaphore::new(
        usize::try_from(max_per_sec).unwrap_or(1) * 2,
    ));

    let server_url: Arc<str> = envs.server_url.clone().into();
    let from_email: Arc<str> = envs.aws_ses_from_email.clone().into();

    spawn_token_refill_task(Arc::clone(&tokens), Arc::clone(&last_refill), max_per_sec);

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

fn spawn_token_refill_task(
    tokens: Arc<AtomicU64>,
    last_refill: Arc<std::sync::Mutex<Instant>>,
    max_per_sec: u64,
) {
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_millis(TOKEN_REFILL_INTERVAL_MS));
        loop {
            interval.tick().await;
            let mut last = last_refill.lock().unwrap();

            if last.elapsed() >= Duration::from_secs(1) {
                tokens.store(max_per_sec, Ordering::SeqCst);
                *last = Instant::now();
            } else {
                let refill = max_per_sec.div_ceil(10);
                let current = tokens.load(Ordering::SeqCst);
                if current < max_per_sec {
                    tokens.store((current + refill).min(max_per_sec), Ordering::SeqCst);
                }
            }
            drop(last);
        }
    });
}

async fn acquire_token(tokens: &AtomicU64) {
    loop {
        let current = tokens.load(Ordering::SeqCst);
        if current > 0
            && tokens
                .compare_exchange(current, current - 1, Ordering::SeqCst, Ordering::SeqCst)
                .is_ok()
        {
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

    let Ok(mut tx) = db_pool.begin().await else {
        error!("Transaction begin failed");
        fallback_individual_updates(db_pool, batch).await;
        return;
    };

    for req in &*batch {
        drop(
            sqlx::query(
                "UPDATE email_requests SET status=?, message_id=?, error=?, updated_at=datetime('now') WHERE id=?",
            )
            .bind(req.status)
            .bind(&req.message_id)
            .bind(&req.error)
            .bind(req.id)
            .execute(&mut *tx)
            .await,
        );
    }

    if let Err(e) = tx.commit().await {
        error!("Transaction commit failed: {e:?}");
    }

    batch.clear();
}

async fn fallback_individual_updates(db_pool: &SqlitePool, batch: &mut Vec<EmailRequest>) {
    for req in batch.drain(..) {
        if let Err(e) = req.update(db_pool).await {
            error!("Update failed for id={:?}: {e:?}", req.id);
        }
    }
}
