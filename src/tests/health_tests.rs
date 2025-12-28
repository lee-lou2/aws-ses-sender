#[cfg(test)]
mod tests {
    use crate::state::AppState;
    use crate::tests::helpers::setup_db;
    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use tower::ServiceExt;

    #[tokio::test]
    async fn test_health_returns_ok() {
        let db = setup_db().await;
        let (tx, _) = tokio::sync::mpsc::channel(1);
        let app = crate::app::app(AppState::new(db, tx));

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/health")
                    .method("GET")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);

        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let parsed: serde_json::Value = serde_json::from_slice(&body).unwrap();

        assert_eq!(parsed["status"], "ok");
    }

    #[tokio::test]
    async fn test_ready_returns_ok_when_db_connected() {
        let db = setup_db().await;
        let (tx, _) = tokio::sync::mpsc::channel(1);
        let app = crate::app::app(AppState::new(db, tx));

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/ready")
                    .method("GET")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);

        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let parsed: serde_json::Value = serde_json::from_slice(&body).unwrap();

        assert_eq!(parsed["status"], "ok");
        assert_eq!(parsed["db"], "connected");
    }

    #[tokio::test]
    async fn test_health_no_auth_required() {
        let db = setup_db().await;
        let (tx, _) = tokio::sync::mpsc::channel(1);
        let app = crate::app::app(AppState::new(db, tx));

        // No X-API-KEY header
        let response = app
            .oneshot(
                Request::builder()
                    .uri("/health")
                    .method("GET")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        // Should still return OK (no auth required)
        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_ready_no_auth_required() {
        let db = setup_db().await;
        let (tx, _) = tokio::sync::mpsc::channel(1);
        let app = crate::app::app(AppState::new(db, tx));

        // No X-API-KEY header
        let response = app
            .oneshot(
                Request::builder()
                    .uri("/ready")
                    .method("GET")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        // Should still return OK (no auth required)
        assert_eq!(response.status(), StatusCode::OK);
    }
}
