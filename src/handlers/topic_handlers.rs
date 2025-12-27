//! Topic management handlers

use crate::{
    models::{request::EmailRequest, result::EmailResult},
    state::AppState,
};
use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
    Json,
};
use tracing::error;

/// Returns email statistics for a specific topic.
pub async fn get_topic(
    State(state): State<AppState>,
    Path(topic_id): Path<String>,
) -> impl IntoResponse {
    if topic_id.is_empty() {
        return (StatusCode::BAD_REQUEST, "topic_id is required").into_response();
    }

    let request_counts =
        match EmailRequest::get_request_counts_by_topic_id(&state.db_pool, &topic_id).await {
            Ok(c) => c,
            Err(e) => {
                error!("Failed to get request counts: {e:?}");
                return (StatusCode::INTERNAL_SERVER_ERROR, "Failed to retrieve data")
                    .into_response();
            }
        };

    let result_counts = match EmailResult::get_result_counts_by_topic_id(&state.db_pool, &topic_id)
        .await
    {
        Ok(c) => c,
        Err(e) => {
            error!("Failed to get result counts: {e:?}");
            return (StatusCode::INTERNAL_SERVER_ERROR, "Failed to retrieve data").into_response();
        }
    };

    let response = serde_json::json!({
        "request_counts": request_counts,
        "result_counts": result_counts,
    });

    (StatusCode::OK, Json(response)).into_response()
}

/// Stops pending emails for a topic (only affects `Created` status).
pub async fn stop_topic(
    State(state): State<AppState>,
    Path(topic_id): Path<String>,
) -> impl IntoResponse {
    match EmailRequest::stop_topic(&state.db_pool, &topic_id).await {
        Ok(()) => (StatusCode::OK, "OK").into_response(),
        Err(e) => {
            error!("Failed to stop topic: {e:?}");
            (StatusCode::INTERNAL_SERVER_ERROR, "Failed to stop topic").into_response()
        }
    }
}
