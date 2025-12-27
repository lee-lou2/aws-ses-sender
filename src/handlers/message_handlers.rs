//! Email message sending handler

use crate::{
    models::request::{EmailMessageStatus, EmailRequest},
    state::AppState,
};
use axum::{extract::State, http::StatusCode, response::IntoResponse, Json};
use serde::{Deserialize, Serialize};
use tracing::{error, info, warn};

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
/// - Immediate: Sent directly to the sending queue
/// - Scheduled: Stored with `scheduled_at` for later processing
pub async fn create_message(
    State(state): State<AppState>,
    Json(payload): Json<CreateMessageRequest>,
) -> impl IntoResponse {
    let start = std::time::Instant::now();

    if payload.messages.is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": "No messages provided"})),
        )
            .into_response();
    }

    let scheduled_at = payload.scheduled_at;
    let is_scheduled = matches!(&scheduled_at, Some(s) if !s.is_empty());
    let status = if is_scheduled {
        EmailMessageStatus::Created as i32
    } else {
        EmailMessageStatus::Processed as i32
    };

    let requests: Vec<EmailRequest> = payload
        .messages
        .into_iter()
        .flat_map(|msg| {
            let topic_id = msg.topic_id.unwrap_or_default();
            let subject = msg.subject;
            let content = msg.content;
            let sched = scheduled_at.clone();

            msg.emails.into_iter().map(move |email| EmailRequest {
                id: None,
                topic_id: Some(topic_id.clone()),
                email,
                subject: subject.clone(),
                content: content.clone(),
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
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({
                "error": format!("Max {MAX_EMAILS_PER_REQUEST} emails per request")
            })),
        )
            .into_response();
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
        for mut req in saved_requests {
            if let Err(e) = state.tx.send(req.clone()).await {
                error!("Failed to send to channel: {e}");
                req.status = EmailMessageStatus::Created as i32;
                if let Err(e) = req.update(&state.db_pool).await {
                    error!("Failed to rollback status for id={:?}: {e}", req.id);
                }
            }
        }
    }

    let duration = start.elapsed();
    info!("Done: {success} ok, {errors} err in {duration:?}");

    (
        StatusCode::OK,
        Json(CreateMessageResponse {
            total,
            success,
            errors,
            duration_ms: duration.as_millis(),
            scheduled: is_scheduled,
        }),
    )
        .into_response()
}
