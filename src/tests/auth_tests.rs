#[cfg(test)]
mod tests {
    use crate::state::AppState;
    use crate::tests::helpers::{get_api_key, setup_db};
    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use tower::ServiceExt;

    #[tokio::test]
    async fn test_valid_api_key() {
        let db = setup_db().await;
        let (tx, _rx) = tokio::sync::mpsc::channel(100);
        let app = crate::app::app(AppState::new(db, tx));

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/v1/events/counts/sent")
                    .method("GET")
                    .header("X-API-KEY", get_api_key())
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_missing_api_key() {
        let db = setup_db().await;
        let (tx, _rx) = tokio::sync::mpsc::channel(100);
        let app = crate::app::app(AppState::new(db, tx));

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/v1/events/counts/sent")
                    .method("GET")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn test_invalid_api_key() {
        let db = setup_db().await;
        let (tx, _rx) = tokio::sync::mpsc::channel(100);
        let app = crate::app::app(AppState::new(db, tx));

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/v1/events/counts/sent")
                    .method("GET")
                    .header("X-API-KEY", "wrong_key")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn test_empty_api_key() {
        let db = setup_db().await;
        let (tx, _rx) = tokio::sync::mpsc::channel(100);
        let app = crate::app::app(AppState::new(db, tx));

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/v1/events/counts/sent")
                    .method("GET")
                    .header("X-API-KEY", "")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn test_public_open_endpoint() {
        let db = setup_db().await;
        let (tx, _rx) = tokio::sync::mpsc::channel(100);
        let app = crate::app::app(AppState::new(db, tx));

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/v1/events/open")
                    .method("GET")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_public_results_endpoint() {
        let db = setup_db().await;
        let (tx, _rx) = tokio::sync::mpsc::channel(100);
        let app = crate::app::app(AppState::new(db, tx));

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/v1/events/results")
                    .method("POST")
                    .header("Content-Type", "application/json")
                    .body(Body::from("{}"))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_ne!(response.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn test_protected_messages_endpoint() {
        let db = setup_db().await;
        let (tx, _rx) = tokio::sync::mpsc::channel(100);
        let app = crate::app::app(AppState::new(db, tx));

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/v1/messages")
                    .method("POST")
                    .header("Content-Type", "application/json")
                    .body(Body::from(r#"{"messages":[]}"#))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn test_protected_topics_get() {
        let db = setup_db().await;
        let (tx, _rx) = tokio::sync::mpsc::channel(100);
        let app = crate::app::app(AppState::new(db, tx));

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/v1/topics/test-topic")
                    .method("GET")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn test_protected_topics_delete() {
        let db = setup_db().await;
        let (tx, _rx) = tokio::sync::mpsc::channel(100);
        let app = crate::app::app(AppState::new(db, tx));

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/v1/topics/test-topic")
                    .method("DELETE")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }
}
