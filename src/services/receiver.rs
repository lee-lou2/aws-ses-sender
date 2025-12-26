use crate::config;
use crate::models::request::{EmailMessageStatus, EmailRequest};
use aws_sdk_sesv2::Client;
use sqlx::SqlitePool;
use std::time::Duration;
use tokio::sync::mpsc;
use tokio::time::interval;

/// receive_send_message
/// Message reception and sending
pub async fn receive_send_message(
    client: Client,
    mut rx: mpsc::Receiver<EmailRequest>,
    tx: mpsc::Sender<EmailRequest>,
) {
    let envs = config::get_environments();
    let max_send_per_second = envs.max_send_per_second;
    // Consume messages based on rate limit
    // Note: This naive interval limits the *pulling* rate.
    // If the queue has backpressure, this effectively limits the sending rate.
    let mut interval = interval(Duration::from_millis(1000 / max_send_per_second as u64));

    loop {
        interval.tick().await;
        if let Some(mut request) = rx.recv().await {
            let server_url = &envs.server_url;
            request.content = format!(
                "{}<img src=\"{}/v1/events/open?request_id={}\">",
                request.content,
                server_url,
                request.id.unwrap_or_default()
            );
            let cloned_tx = tx.clone();
            let client = client.clone();
            let aws_ses_from_email = envs.aws_ses_from_email.clone();

            tokio::spawn(async move {
                let send_result = crate::services::sender::send_email(
                    &client,
                    &aws_ses_from_email,
                    &request.email,
                    &request.subject,
                    &request.content,
                )
                .await;

                match send_result {
                    Ok(message_id) => {
                        request.status = EmailMessageStatus::Sent as i32;
                        request.message_id = Some(message_id);
                    }
                    Err(e) => {
                        request.status = EmailMessageStatus::Failed as i32;
                        request.error = Some(format!("Failed to send email: {}", e));
                    }
                }
                if let Err(e) = cloned_tx.send(request).await {
                    tracing::error!("Error sending data to channel: {:?}", e);
                } else {
                    tracing::debug!("Data sent to channel");
                }
            });
        } else {
            break;
        }
    }
}

/// receive_post_send_message
/// Update the database with received message results
pub async fn receive_post_send_message(
    mut rx: mpsc::Receiver<EmailRequest>,
    db_pool: SqlitePool,
) {
    let mut buffer = Vec::with_capacity(50);
    // Timeout duration for flushing the buffer
    let timeout_duration = Duration::from_millis(500);

    loop {
        let receive_future = rx.recv();
        tokio::select! {
            result = receive_future => {
                match result {
                    Some(request) => {
                        buffer.push(request);
                        if buffer.len() >= 50 {
                            flush_buffer(&mut buffer, &db_pool).await;
                        }
                    }
                    None => {
                        // Channel closed, flush remaining and exit
                        if !buffer.is_empty() {
                            flush_buffer(&mut buffer, &db_pool).await;
                        }
                        break;
                    }
                }
            }
            _ = tokio::time::sleep(timeout_duration), if !buffer.is_empty() => {
                flush_buffer(&mut buffer, &db_pool).await;
            }
        }
    }
}

async fn flush_buffer(buffer: &mut Vec<EmailRequest>, db_pool: &SqlitePool) {
    if buffer.is_empty() {
        return;
    }

    let mut transaction = match db_pool.begin().await {
        Ok(tx) => tx,
        Err(e) => {
            tracing::error!("Failed to start transaction: {:?}", e);
            // If we can't start a transaction, try updating individually to not lose data?
            // Or just retry? For now, we just log and try individually without transaction which might also fail.
            // Let's try to process them one by one as fallback.
             for request in buffer.drain(..) {
                request.update(db_pool).await;
            }
            return;
        }
    };

    // We process the buffer.
    // Since EmailRequest::update uses its own query execution against db_pool,
    // we need to change EmailRequest::update to accept a transaction or connection.
    // However, changing the model method might be invasive.
    // For now, I will reimplement the update logic here or just loop.
    // BUT: To use the transaction, I must execute queries ON the transaction.
    // EmailRequest::update takes &SqlitePool.
    // I can't easily pass the transaction to it unless I overload it.
    // Let's perform the updates manually here for now, or use `request.update_with_executor` if I add it.
    // Given the constraints, I will copy the update logic here to ensure it uses the transaction.

    for request in buffer.drain(..) {
         let result = sqlx::query!(
            r#"
            UPDATE email_requests
            SET status = ?,
                message_id = ?,
                error = ?,
                updated_at = datetime('now')
            WHERE id = ?
            "#,
            request.status,
            request.message_id,
            request.error,
            request.id,
        )
        .execute(&mut *transaction)
        .await;

        if let Err(e) = result {
             tracing::error!("Failed to update request {}: {:?}", request.id.unwrap_or(-1), e);
             // If one fails, we might still want to commit the others?
             // But if we error here, the transaction is poisoned?
             // SQLite doesn't support nested transactions easily.
             // If one fails, we probably should rollback? Or just ignore?
             // Let's log.
        }
    }

    if let Err(e) = transaction.commit().await {
        tracing::error!("Failed to commit transaction: {:?}", e);
    } else {
        tracing::info!("Batch updated emails.");
    }
}
