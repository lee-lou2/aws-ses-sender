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
) -> impl IntoResponse {
    let hours = query.hours.unwrap_or(24);

    match EmailRequest::sent_count(&state.db_pool, hours).await {
        Ok(count) => (StatusCode::OK, Json(SentCountResponse { count })).into_response(),
        Err(e) => {
            error!("Failed to get sent count: {e:?}");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                "Failed to retrieve count",
            )
                .into_response()
        }
    }
}

/// Handles AWS SNS events (Bounce, Complaint, Delivery, etc.).
pub async fn handle_sns_event(
    State(state): State<AppState>,
    request: Request,
) -> impl IntoResponse {
    let msg_type = request
        .headers()
        .get("x-amz-sns-message-type")
        .and_then(|v| v.to_str().ok());

    if !matches!(msg_type, Some("Notification" | "SubscriptionConfirmation")) {
        error!("Invalid SNS message type");
        return (StatusCode::BAD_REQUEST, "Invalid SNS Message Type").into_response();
    }

    let Ok(body) = axum::body::to_bytes(request.into_body(), MAX_BODY_SIZE).await else {
        error!("Failed to read body");
        return (StatusCode::BAD_REQUEST, "Failed to read body").into_response();
    };

    let Ok(sns_msg) = serde_json::from_slice::<SnsMessage>(&body) else {
        error!("Failed to parse SNS message");
        return (StatusCode::BAD_REQUEST, "Failed to parse message").into_response();
    };

    match sns_msg {
        SnsMessage::SubscriptionConfirmation { subscribe_url } => {
            info!("Subscription confirmation: {subscribe_url}");
            (StatusCode::OK, "Subscription confirmation required").into_response()
        }
        SnsMessage::Notification {
            message,
            message_id,
        } => process_ses_notification(&state, &message, &message_id).await,
        SnsMessage::Other(_) => {
            info!("Other message type received");
            (StatusCode::OK, "OK").into_response()
        }
    }
}

#[allow(clippy::similar_names)]
async fn process_ses_notification(
    state: &AppState,
    message: &str,
    sns_message_id: &str,
) -> axum::response::Response {
    let Ok(notification) = serde_json::from_str::<SesNotification>(message) else {
        error!("Failed to parse SES notification");
        return (StatusCode::OK, "Non-SES notification").into_response();
    };

    let ses_msg_id = notification
        .other_fields
        .get("mail")
        .and_then(|m| m.get("messageId"))
        .and_then(Value::as_str);

    let Some(ses_msg_id) = ses_msg_id else {
        error!("SES message_id not found. SNS: {sns_message_id}");
        return (StatusCode::BAD_REQUEST, "SES message_id not found").into_response();
    };

    let request_id =
        match EmailRequest::get_request_id_by_message_id(&state.db_pool, ses_msg_id).await {
            Ok(id) => id,
            Err(e) => {
                error!("Request lookup failed. SES: {ses_msg_id}, Error: {e:?}");
                return (StatusCode::INTERNAL_SERVER_ERROR, "Request not found").into_response();
            }
        };

    let result = EmailResult {
        id: None,
        request_id,
        status: notification.event_type,
        raw: Some(message.to_owned()),
    };

    match result.save(&state.db_pool).await {
        Ok(_) => (StatusCode::OK, "OK").into_response(),
        Err(e) => {
            error!("Failed to save event: {e:?}");
            (StatusCode::INTERNAL_SERVER_ERROR, "Failed to save").into_response()
        }
    }
}
