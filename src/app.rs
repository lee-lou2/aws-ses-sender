//! HTTP routing configuration

use axum::{
    middleware::from_fn,
    routing::{delete, get, post},
    Router,
};
use tower_http::trace::TraceLayer;

use crate::{handlers, middlewares, state};

/// Creates the Axum router with all routes configured.
pub fn app(state: state::AppState) -> Router {
    let auth = from_fn(middlewares::auth_middlewares::api_key_auth);

    Router::new()
        // Health check endpoints (no auth required)
        .route("/health", get(handlers::health_handlers::health))
        .route("/ready", get(handlers::health_handlers::ready))
        // API endpoints
        .route(
            "/v1/messages",
            post(handlers::message_handlers::create_message).layer(auth.clone()),
        )
        .route(
            "/v1/topics/{topic_id}",
            get(handlers::topic_handlers::get_topic).layer(auth.clone()),
        )
        .route(
            "/v1/topics/{topic_id}",
            delete(handlers::topic_handlers::stop_topic).layer(auth.clone()),
        )
        .route("/v1/events/open", get(handlers::event_handlers::track_open))
        .route(
            "/v1/events/counts/sent",
            get(handlers::event_handlers::get_sent_count).layer(auth),
        )
        .route(
            "/v1/events/results",
            post(handlers::event_handlers::handle_sns_event),
        )
        .with_state(state)
        .layer(TraceLayer::new_for_http())
}
