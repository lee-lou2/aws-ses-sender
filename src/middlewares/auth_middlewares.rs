//! API key authentication middleware

use axum::{
    body::Body,
    http::{Request, StatusCode},
    middleware::Next,
    response::IntoResponse,
};

const API_KEY_HEADER: &str = "X-API-KEY";

/// Validates the `X-API-KEY` header against the configured API key.
pub async fn api_key_auth(req: Request<Body>, next: Next) -> impl IntoResponse {
    let api_key = req
        .headers()
        .get(API_KEY_HEADER)
        .and_then(|v| v.to_str().ok());

    let Some(key) = api_key else {
        return (StatusCode::UNAUTHORIZED, "Missing X-API-KEY header").into_response();
    };

    let expected_key = &crate::config::get_environments().api_key;

    if key != expected_key {
        return (StatusCode::UNAUTHORIZED, "Invalid API Key").into_response();
    }

    next.run(req).await
}
