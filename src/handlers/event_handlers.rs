//! Event tracking and SNS webhook handlers

use axum::{
    extract::{Json, Query, Request, State},
    http::{header::HeaderValue, HeaderMap, StatusCode},
    response::IntoResponse,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tracing::{error, info};

use crate::{
    error::{AppError, AppResult},
    models::{request::EmailRequest, result::EmailResult},
    state::AppState,
};

const MAX_BODY_SIZE: usize = 1024 * 1024; // 1MB

/// 1x1 transparent PNG for email open tracking
const TRACKING_PIXEL: &[u8] = &[
    0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x48, 0x44, 0x52,
    0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x06, 0x00, 0x00, 0x00, 0x1F, 0x15, 0xC4,
    0x89, 0x00, 0x00, 0x00, 0x0A, 0x49, 0x44, 0x41, 0x54, 0x78, 0x9C, 0x63, 0x00, 0x00, 0x00, 0x02,
    0x00, 0x01, 0xE2, 0x26, 0x05, 0x9B, 0x00, 0x00, 0x00, 0x00, 0x49, 0x45, 0x4E, 0x44, 0xAE, 0x42,
    0x60, 0x82,
];

#[derive(Debug, Deserialize)]
pub struct OpenQueryParams {
    pub request_id: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct SentCountQueryParams {
    pub hours: Option<i32>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct SentCountResponse {
    pub count: i32,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(untagged)]
enum SnsMessage {
    SubscriptionConfirmation {
        #[serde(rename = "SubscribeURL")]
        subscribe_url: String,
    },
    Notification {
        #[serde(rename = "Message")]
        message: String,
        #[serde(rename = "MessageId")]
        message_id: String,
    },
    Other(Value),
}

#[derive(Debug, Deserialize)]
struct SesNotification {
    #[serde(rename = "notificationType")]
    event_type: String,
    #[serde(flatten)]
    other_fields: Value,
}

/// Tracks email opens and returns a 1x1 transparent PNG.
pub async fn track_open(
    State(state): State<AppState>,
    Query(query): Query<OpenQueryParams>,
) -> impl IntoResponse {
    if let Some(ref id_str) = query.request_id {
        if let Ok(id) = id_str.parse::<i32>() {
            let result = EmailResult {
                id: None,
                status: "Open".to_owned(),
                request_id: id,
                raw: None,
            };
            if let Err(e) = result.save(&state.db_pool).await {
                error!("Failed to save open event: {e:?}");
            }
        }
    }

    let mut headers = HeaderMap::new();
    headers.insert("Content-Type", HeaderValue::from_static("image/png"));
    (StatusCode::OK, headers, TRACKING_PIXEL)
}

/// Returns the count of emails sent within the specified hours.
pub async fn get_sent_count(
    State(state): State<AppState>,
    Query(query): Query<SentCountQueryParams>,
) -> AppResult<impl IntoResponse> {
    let hours = query.hours.unwrap_or(24);
    let count = EmailRequest::sent_count(&state.db_pool, hours).await?;
    Ok(Json(SentCountResponse { count }))
}

/// Handles AWS SNS events (Bounce, Complaint, Delivery, etc.).
pub async fn handle_sns_event(
    State(state): State<AppState>,
    request: Request,
) -> AppResult<impl IntoResponse> {
    let msg_type = request
        .headers()
        .get("x-amz-sns-message-type")
        .and_then(|v| v.to_str().ok());

    if !matches!(msg_type, Some("Notification" | "SubscriptionConfirmation")) {
        return Err(AppError::BadRequest("Invalid SNS Message Type".to_string()));
    }

    let body = axum::body::to_bytes(request.into_body(), MAX_BODY_SIZE)
        .await
        .map_err(|_| AppError::BadRequest("Failed to read body".to_string()))?;

    let sns_msg: SnsMessage = serde_json::from_slice(&body)
        .map_err(|_| AppError::BadRequest("Failed to parse message".to_string()))?;

    match sns_msg {
        SnsMessage::SubscriptionConfirmation { subscribe_url } => {
            info!("Subscription confirmation: {subscribe_url}");
            Ok(Json(
                serde_json::json!({"status": "subscription_confirmation_required"}),
            ))
        }
        SnsMessage::Notification {
            message,
            message_id,
        } => {
            process_ses_notification(&state, &message, &message_id).await?;
            Ok(Json(serde_json::json!({"status": "ok"})))
        }
        SnsMessage::Other(_) => {
            info!("Other message type received");
            Ok(Json(serde_json::json!({"status": "ok"})))
        }
    }
}

#[allow(clippy::similar_names)]
async fn process_ses_notification(
    state: &AppState,
    message: &str,
    sns_message_id: &str,
) -> AppResult<()> {
    let notification: SesNotification = serde_json::from_str(message)
        .map_err(|_| AppError::BadRequest("Non-SES notification".to_string()))?;

    let ses_msg_id = notification
        .other_fields
        .get("mail")
        .and_then(|m| m.get("messageId"))
        .and_then(Value::as_str);

    let ses_msg_id = ses_msg_id.ok_or_else(|| {
        error!("SES message_id not found. SNS: {sns_message_id}");
        AppError::BadRequest("SES message_id not found".to_string())
    })?;

    let request_id = EmailRequest::get_request_id_by_message_id(&state.db_pool, ses_msg_id)
        .await
        .map_err(|e| {
            error!("Request lookup failed. SES: {ses_msg_id}, Error: {e:?}");
            AppError::NotFound("Request not found".to_string())
        })?;

    let result = EmailResult {
        id: None,
        request_id,
        status: notification.event_type,
        raw: Some(message.to_owned()),
    };

    result.save(&state.db_pool).await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tracking_pixel_is_valid_png() {
        // PNG signature check
        assert_eq!(
            &TRACKING_PIXEL[0..8],
            &[0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A]
        );
    }

    #[test]
    fn test_sent_count_response_serialization() {
        let response = SentCountResponse { count: 42 };
        let json = serde_json::to_string(&response).unwrap();
        assert!(json.contains("42"));
    }

    #[test]
    fn test_open_query_params_deserialization() {
        let json = r#"{"request_id": "123"}"#;
        let params: OpenQueryParams = serde_json::from_str(json).unwrap();
        assert_eq!(params.request_id, Some("123".to_string()));
    }

    #[test]
    fn test_sns_message_notification_deserialization() {
        let json = r#"{"Message": "test", "MessageId": "msg-123"}"#;
        let msg: SnsMessage = serde_json::from_str(json).unwrap();
        assert!(matches!(msg, SnsMessage::Notification { .. }));
    }

    #[test]
    fn test_sns_message_subscription_confirmation_deserialization() {
        let json = r#"{"SubscribeURL": "https://example.com/confirm"}"#;
        let msg: SnsMessage = serde_json::from_str(json).unwrap();
        assert!(matches!(msg, SnsMessage::SubscriptionConfirmation { .. }));
    }
}
