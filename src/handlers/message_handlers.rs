//! Email message sending handler

use std::sync::Arc;

use axum::{extract::State, response::IntoResponse, Json};
use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;
use tracing::{error, info, warn};

use crate::{
    error::{AppError, AppResult},
    models::{
        content::EmailContent,
        request::{EmailMessageStatus, EmailRequest},
    },
    state::AppState,
};

const MAX_EMAILS_PER_REQUEST: usize = 10_000;

#[derive(Debug, Deserialize)]
pub struct Message {
    pub topic_id: Option<String>,
    pub emails: Vec<String>,
    pub subject: String,
    pub content: String,
}

#[derive(Debug, Deserialize)]
pub struct CreateMessageRequest {
    pub messages: Vec<Message>,
    pub scheduled_at: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct CreateMessageResponse {
    pub total: usize,
    pub success: usize,
    pub errors: usize,
    pub duration_ms: u128,
    pub scheduled: bool,
}

/// Creates email sending requests.
///
/// 1. Saves content (subject, body) to `email_contents` table
/// 2. Creates requests with `content_id` reference
/// - Immediate: Sent directly to the sending queue
/// - Scheduled: Stored with `scheduled_at` for later processing
#[allow(clippy::too_many_lines)]
pub async fn create_message(
    State(state): State<AppState>,
    Json(payload): Json<CreateMessageRequest>,
) -> AppResult<impl IntoResponse> {
    let start = std::time::Instant::now();

    if payload.messages.is_empty() {
        return Err(AppError::BadRequest("No messages provided".to_string()));
    }

    let scheduled_at = payload.scheduled_at;
    let is_scheduled = matches!(&scheduled_at, Some(s) if !s.is_empty());
    let status = if is_scheduled {
        EmailMessageStatus::Created as i32
    } else {
        EmailMessageStatus::Processed as i32
    };

    // 1. Save contents first (one per message)
    let contents: Vec<EmailContent> = payload
        .messages
        .iter()
        .map(|msg| EmailContent {
            id: None,
            subject: msg.subject.clone(),
            content: msg.content.clone(),
        })
        .collect();

    let saved_contents = EmailContent::save_batch(contents, &state.db_pool).await?;

    // 2. Create requests with content_id
    // Use Arc to share subject/content across all emails in the same message,
    // avoiding expensive string cloning (e.g., 10,000 emails = 1 Arc::clone vs 10,000 String::clone)
    let requests: Vec<EmailRequest> = payload
        .messages
        .into_iter()
        .zip(saved_contents.iter())
        .flat_map(|(msg, saved_content)| {
            let topic_id = msg.topic_id.unwrap_or_default();
            let content_id = saved_content.id;
            // Create Arc once per message, share across all emails
            let subject = Arc::new(saved_content.subject.clone());
            let content = Arc::new(saved_content.content.clone());
            let sched = scheduled_at.clone();

            msg.emails.into_iter().map(move |email| EmailRequest {
                id: None,
                topic_id: Some(topic_id.clone()),
                content_id,
                email,
                subject: Arc::clone(&subject),
                content: Arc::clone(&content),
                scheduled_at: sched.clone(),
                status,
                error: None,
                message_id: None,
            })
        })
        .collect();

    let total = requests.len();

    if total > MAX_EMAILS_PER_REQUEST {
        warn!("Too many emails: {total} > {MAX_EMAILS_PER_REQUEST}");
        return Err(AppError::BadRequest(format!(
            "Max {MAX_EMAILS_PER_REQUEST} emails per request"
        )));
    }

    info!("Processing {total} emails (scheduled={is_scheduled})");

    let (success, errors, saved_requests) =
        match EmailRequest::save_batch(requests, &state.db_pool).await {
            Ok(saved) => {
                let count = saved.len();
                (count, 0, saved)
            }
            Err(e) => {
                error!("Batch save failed: {e:?}");
                (0, total, Vec::new())
            }
        };

    if !is_scheduled {
        let mut failed_requests = Vec::new();
        let mut channel_closed = false;

        for req in saved_requests {
            if channel_closed {
                // Channel already closed, add remaining to failed
                let mut req = req;
                req.status = EmailMessageStatus::Created as i32;
                failed_requests.push(req);
                continue;
            }

            // try_send for non-blocking channel send when buffer is available
            match state.tx.try_send(req) {
                Ok(()) => {}
                Err(tokio::sync::mpsc::error::TrySendError::Full(req)) => {
                    // Channel full: fall back to blocking send
                    if state.tx.send(req).await.is_err() {
                        error!("Channel closed while sending");
                        channel_closed = true;
                    }
                }
                Err(tokio::sync::mpsc::error::TrySendError::Closed(mut req)) => {
                    error!("Channel closed");
                    req.status = EmailMessageStatus::Created as i32;
                    failed_requests.push(req);
                    channel_closed = true;
                }
            }
        }

        // Batch rollback for failed requests
        if !failed_requests.is_empty() {
            warn!("Rolling back {} failed requests", failed_requests.len());
            let ids: Vec<i32> = failed_requests.iter().filter_map(|r| r.id).collect();
            if let Err(e) = rollback_to_created(&state.db_pool, &ids).await {
                error!("Failed to rollback {} requests: {e}", ids.len());
            }
        }
    }

    let duration = start.elapsed();
    info!("Done: {success} ok, {errors} err in {duration:?}");

    Ok(Json(CreateMessageResponse {
        total,
        success,
        errors,
        duration_ms: duration.as_millis(),
        scheduled: is_scheduled,
    }))
}

/// Batch rollback requests to Created status when channel send fails.
async fn rollback_to_created(db_pool: &SqlitePool, ids: &[i32]) -> Result<(), sqlx::Error> {
    if ids.is_empty() {
        return Ok(());
    }

    let placeholders = vec!["?"; ids.len()].join(",");
    let sql = format!(
        "UPDATE email_requests SET status=?, updated_at=datetime('now') WHERE id IN ({placeholders})"
    );

    let mut query = sqlx::query(&sql).bind(EmailMessageStatus::Created as i32);
    for id in ids {
        query = query.bind(id);
    }
    query.execute(db_pool).await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_create_message_response_serialization() {
        let response = CreateMessageResponse {
            total: 100,
            success: 98,
            errors: 2,
            duration_ms: 150,
            scheduled: false,
        };

        let json = serde_json::to_string(&response).unwrap();
        assert!(json.contains("\"total\":100"));
        assert!(json.contains("\"success\":98"));
        assert!(json.contains("\"errors\":2"));
    }

    #[test]
    fn test_message_deserialization() {
        let json = r#"{
            "topic_id": "topic-1",
            "emails": ["test@example.com"],
            "subject": "Hello",
            "content": "<p>World</p>"
        }"#;

        let msg: Message = serde_json::from_str(json).unwrap();
        assert_eq!(msg.topic_id, Some("topic-1".to_string()));
        assert_eq!(msg.emails.len(), 1);
    }

    #[test]
    fn test_create_message_request_deserialization() {
        let json = r#"{
            "messages": [{
                "emails": ["test@example.com"],
                "subject": "Test",
                "content": "Content"
            }],
            "scheduled_at": "2025-01-01 12:00:00"
        }"#;

        let req: CreateMessageRequest = serde_json::from_str(json).unwrap();
        assert_eq!(req.messages.len(), 1);
        assert!(req.scheduled_at.is_some());
    }
}
