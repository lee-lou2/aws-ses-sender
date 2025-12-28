//! API key authentication middleware

use axum::{
    body::Body,
    http::{Request, StatusCode},
    middleware::Next,
    response::IntoResponse,
    Json,
};
use serde_json::json;

use crate::config::APP_CONFIG;

const API_KEY_HEADER: &str = "X-API-KEY";

/// Validates the `X-API-KEY` header against the configured API key.
pub async fn api_key_auth(req: Request<Body>, next: Next) -> impl IntoResponse {
    let api_key = req
        .headers()
        .get(API_KEY_HEADER)
        .and_then(|v| v.to_str().ok());

    let Some(key) = api_key else {
        return (
            StatusCode::UNAUTHORIZED,
            Json(json!({"error": "Missing X-API-KEY header"})),
        )
            .into_response();
    };

    if key.is_empty() {
        return (
            StatusCode::UNAUTHORIZED,
            Json(json!({"error": "Empty API Key"})),
        )
            .into_response();
    }

    let expected_key = &APP_CONFIG.api_key;

    if expected_key.is_empty() || key != expected_key {
        return (
            StatusCode::UNAUTHORIZED,
            Json(json!({"error": "Invalid API Key"})),
        )
            .into_response();
    }

    next.run(req).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_api_key_header_constant() {
        assert_eq!(API_KEY_HEADER, "X-API-KEY");
    }
}
