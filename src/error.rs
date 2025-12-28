//! 중앙화된 에러 처리 모듈.

use axum::{
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde_json::json;
use thiserror::Error;

/// Application-wide error type.
///
/// All errors in the application should be converted to this type
/// for consistent error handling and reporting.
#[derive(Error, Debug)]
#[allow(dead_code)]
pub enum AppError {
    /// Bad request error (400)
    #[error("Bad request: {0}")]
    BadRequest(String),

    /// Unauthorized error (401)
    #[error("Unauthorized: {0}")]
    Unauthorized(String),

    /// Not found error (404)
    #[error("Not found: {0}")]
    NotFound(String),

    /// Validation error (400)
    #[error("Validation error: {0}")]
    Validation(String),

    /// Internal server error (500)
    #[error("Internal server error: {0}")]
    Internal(String),

    /// Database error
    #[error("Database error: {0}")]
    Database(#[from] sqlx::Error),

    /// Email sending error
    #[error("Email error: {0}")]
    Email(String),

    /// Channel send error
    #[error("Channel closed")]
    ChannelClosed,
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let (status, error_message) = match &self {
            Self::BadRequest(msg) | Self::Validation(msg) => (StatusCode::BAD_REQUEST, msg.clone()),
            Self::Unauthorized(msg) => (StatusCode::UNAUTHORIZED, msg.clone()),
            Self::NotFound(msg) => (StatusCode::NOT_FOUND, msg.clone()),
            Self::Internal(msg) => (StatusCode::INTERNAL_SERVER_ERROR, msg.clone()),
            Self::Database(e) => {
                tracing::error!("Database error: {e:?}");
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "Database error occurred".to_string(),
                )
            }
            Self::Email(msg) => {
                tracing::error!("Email error: {msg}");
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "Email sending error".to_string(),
                )
            }
            Self::ChannelClosed => {
                tracing::error!("Channel closed unexpectedly");
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "Internal channel error".to_string(),
                )
            }
        };

        // Report error to Sentry for server errors
        if status.is_server_error() {
            sentry::capture_error(&self);
        }

        let body = Json(json!({
            "error": error_message,
        }));

        (status, body).into_response()
    }
}

/// Result type alias using `AppError`.
pub type AppResult<T> = Result<T, AppError>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_app_error_bad_request_display() {
        let error = AppError::BadRequest("잘못된 요청".to_string());
        assert_eq!(error.to_string(), "Bad request: 잘못된 요청");
    }

    #[test]
    fn test_app_error_unauthorized_display() {
        let error = AppError::Unauthorized("인증 실패".to_string());
        assert_eq!(error.to_string(), "Unauthorized: 인증 실패");
    }

    #[test]
    fn test_app_error_not_found_display() {
        let error = AppError::NotFound("리소스를 찾을 수 없음".to_string());
        assert_eq!(error.to_string(), "Not found: 리소스를 찾을 수 없음");
    }

    #[test]
    fn test_app_error_validation_display() {
        let error = AppError::Validation("유효성 검사 실패".to_string());
        assert_eq!(error.to_string(), "Validation error: 유효성 검사 실패");
    }

    #[test]
    fn test_app_error_internal_display() {
        let error = AppError::Internal("내부 오류".to_string());
        assert_eq!(error.to_string(), "Internal server error: 내부 오류");
    }

    #[test]
    fn test_app_error_email_display() {
        let error = AppError::Email("발송 실패".to_string());
        assert_eq!(error.to_string(), "Email error: 발송 실패");
    }

    #[test]
    fn test_app_error_channel_closed_display() {
        let error = AppError::ChannelClosed;
        assert_eq!(error.to_string(), "Channel closed");
    }

    #[test]
    fn test_app_error_debug_format() {
        let error = AppError::BadRequest("test".to_string());
        let debug_str = format!("{error:?}");
        assert!(debug_str.contains("BadRequest"));
        assert!(debug_str.contains("test"));
    }

    #[tokio::test]
    async fn test_bad_request_into_response() {
        let error = AppError::BadRequest("테스트 에러".to_string());
        let response = error.into_response();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn test_unauthorized_into_response() {
        let error = AppError::Unauthorized("인증 필요".to_string());
        let response = error.into_response();
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn test_not_found_into_response() {
        let error = AppError::NotFound("없음".to_string());
        let response = error.into_response();
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn test_validation_into_response() {
        let error = AppError::Validation("유효하지 않음".to_string());
        let response = error.into_response();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn test_internal_into_response() {
        let error = AppError::Internal("서버 오류".to_string());
        let response = error.into_response();
        assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
    }

    #[test]
    fn test_app_result_ok() {
        let value = 42;
        let result: AppResult<i32> = Ok(value);
        assert!(result.is_ok());
        assert_eq!(result.ok(), Some(value));
    }

    #[test]
    fn test_app_result_err() {
        let result: AppResult<i32> = Err(AppError::NotFound("테스트".to_string()));
        assert!(result.is_err());
    }

    #[test]
    fn test_error_empty_message() {
        let error = AppError::BadRequest(String::new());
        assert_eq!(error.to_string(), "Bad request: ");
    }

    #[test]
    fn test_error_unicode_message() {
        let error = AppError::NotFound("🔍 찾을 수 없습니다".to_string());
        assert!(error.to_string().contains("🔍"));
    }

    #[tokio::test]
    async fn test_error_response_has_body() {
        use axum::body::to_bytes;

        let error = AppError::NotFound("resource not found".to_string());
        let response = error.into_response();

        let body = to_bytes(response.into_body(), 1024).await.unwrap();
        let body_str = String::from_utf8_lossy(&body);

        assert!(body_str.contains("error"));
        assert!(body_str.contains("resource not found"));
    }

    #[tokio::test]
    async fn test_error_response_is_json() {
        use axum::body::to_bytes;

        let error = AppError::BadRequest("test".to_string());
        let response = error.into_response();

        let body = to_bytes(response.into_body(), 1024).await.unwrap();
        let parsed: serde_json::Value = serde_json::from_slice(&body).unwrap();

        assert!(parsed.get("error").is_some());
    }

    #[tokio::test]
    async fn test_all_error_types_produce_valid_response() {
        let errors: Vec<AppError> = vec![
            AppError::BadRequest("bad".to_string()),
            AppError::Unauthorized("unauth".to_string()),
            AppError::NotFound("not found".to_string()),
            AppError::Validation("invalid".to_string()),
            AppError::Internal("internal".to_string()),
            AppError::Email("email error".to_string()),
            AppError::ChannelClosed,
        ];

        for error in errors {
            let response = error.into_response();
            assert!(response.status().is_client_error() || response.status().is_server_error());
        }
    }
}
