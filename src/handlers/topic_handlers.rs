//! Topic management handlers

use axum::{
    extract::{Path, State},
    response::IntoResponse,
    Json,
};

use crate::{
    error::{AppError, AppResult},
    models::{request::EmailRequest, result::EmailResult},
    state::AppState,
};

/// Topic statistics response.
#[derive(serde::Serialize)]
struct TopicStatsResponse {
    request_counts: std::collections::HashMap<String, i32>,
    result_counts: std::collections::HashMap<String, i32>,
}

/// Returns email statistics for a specific topic.
///
/// Executes request and result count queries in parallel for better performance.
pub async fn get_topic(
    State(state): State<AppState>,
    Path(topic_id): Path<String>,
) -> AppResult<impl IntoResponse> {
    if topic_id.is_empty() {
        return Err(AppError::BadRequest("topic_id is required".to_string()));
    }

    // Execute both queries in parallel
    let (request_result, result_result) = tokio::join!(
        EmailRequest::get_request_counts_by_topic_id(&state.db_pool, &topic_id),
        EmailResult::get_result_counts_by_topic_id(&state.db_pool, &topic_id)
    );

    let request_counts = request_result?;
    let result_counts = result_result?;

    Ok(Json(TopicStatsResponse {
        request_counts,
        result_counts,
    }))
}

/// Stops pending emails for a topic (only affects `Created` status).
pub async fn stop_topic(
    State(state): State<AppState>,
    Path(topic_id): Path<String>,
) -> AppResult<impl IntoResponse> {
    if topic_id.is_empty() {
        return Err(AppError::BadRequest("topic_id is required".to_string()));
    }

    EmailRequest::stop_topic(&state.db_pool, &topic_id).await?;
    Ok(Json(serde_json::json!({"status": "ok"})))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_topic_stats_response_serialization() {
        let mut request_counts = std::collections::HashMap::new();
        request_counts.insert("Sent".to_string(), 10);

        let mut result_counts = std::collections::HashMap::new();
        result_counts.insert("Delivery".to_string(), 8);

        let response = TopicStatsResponse {
            request_counts,
            result_counts,
        };

        let json = serde_json::to_string(&response).unwrap();
        assert!(json.contains("request_counts"));
        assert!(json.contains("result_counts"));
    }
}
