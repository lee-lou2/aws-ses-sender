#[cfg(test)]
mod tests {
    use crate::models::request::{EmailMessageStatus, EmailRequest};
    use crate::models::result::EmailResult;
    use crate::state::AppState;
    use crate::tests::helpers::{
        get_api_key, insert_default_content, insert_request_raw, insert_request_with_id, setup_db,
    };
    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use tower::ServiceExt;

    #[tokio::test]
    async fn test_get_topic_empty() {
        let db = setup_db().await;
        let (tx, _rx) = tokio::sync::mpsc::channel(100);
        let app = crate::app::app(AppState::new(db, tx));

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/v1/topics/empty-topic")
                    .method("GET")
                    .header("X-API-KEY", get_api_key())
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

        assert!(parsed["request_counts"].as_object().unwrap().is_empty());
        assert!(parsed["result_counts"].as_object().unwrap().is_empty());
    }

    #[tokio::test]
    async fn test_get_topic_with_results() {
        let db = setup_db().await;
        let content_id = insert_default_content(&db).await;

        // Insert request with explicit id
        insert_request_with_id(
            &db,
            1,
            content_id,
            "result-topic",
            "test@test.com",
            2,
            Some("msg-123"),
        )
        .await;

        // Insert results
        EmailResult {
            id: None,
            request_id: 1,
            status: "Delivery".to_string(),
            raw: None,
        }
        .save(&db)
        .await
        .unwrap();

        EmailResult {
            id: None,
            request_id: 1,
            status: "Open".to_string(),
            raw: None,
        }
        .save(&db)
        .await
        .unwrap();

        let (tx, _rx) = tokio::sync::mpsc::channel(100);
        let app = crate::app::app(AppState::new(db, tx));

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/v1/topics/result-topic")
                    .method("GET")
                    .header("X-API-KEY", get_api_key())
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

        assert_eq!(parsed["request_counts"]["Sent"], 1);
        assert_eq!(parsed["result_counts"]["Delivery"], 1);
        assert_eq!(parsed["result_counts"]["Open"], 1);
    }

    #[tokio::test]
    async fn test_stop_topic_empty() {
        let db = setup_db().await;
        let (tx, _rx) = tokio::sync::mpsc::channel(100);
        let app = crate::app::app(AppState::new(db.clone(), tx));

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/v1/topics/nonexistent-topic")
                    .method("DELETE")
                    .header("X-API-KEY", get_api_key())
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);

        let count: (i32,) = sqlx::query_as("SELECT COUNT(*) FROM email_requests WHERE status = ?")
            .bind(EmailMessageStatus::Stopped as i32)
            .fetch_one(&db)
            .await
            .unwrap();

        assert_eq!(count.0, 0);
    }

    #[tokio::test]
    async fn test_stop_topic_multiple_statuses() {
        let db = setup_db().await;
        let content_id = insert_default_content(&db).await;

        // Created (should be stopped)
        insert_request_raw(&db, content_id, "multi-status", "a@test.com", 0, None).await;

        // Processed (should NOT be stopped)
        insert_request_raw(&db, content_id, "multi-status", "b@test.com", 1, None).await;

        // Sent (should NOT be stopped)
        insert_request_raw(&db, content_id, "multi-status", "c@test.com", 2, None).await;

        // Failed (should NOT be stopped)
        insert_request_raw(&db, content_id, "multi-status", "d@test.com", 3, None).await;

        let (tx, _rx) = tokio::sync::mpsc::channel(100);
        let app = crate::app::app(AppState::new(db.clone(), tx));

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/v1/topics/multi-status")
                    .method("DELETE")
                    .header("X-API-KEY", get_api_key())
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);

        // Verify only Created was stopped
        let counts = EmailRequest::get_request_counts_by_topic_id(&db, "multi-status")
            .await
            .unwrap();

        assert_eq!(counts.get("Stopped"), Some(&1));
        assert_eq!(counts.get("Processed"), Some(&1));
        assert_eq!(counts.get("Sent"), Some(&1));
        assert_eq!(counts.get("Failed"), Some(&1));
    }

    #[tokio::test]
    async fn test_get_topic_all_status_types() {
        let db = setup_db().await;
        let content_id = insert_default_content(&db).await;

        // Insert one of each status
        for (status, email) in [
            (0, "created@test.com"),
            (1, "processed@test.com"),
            (2, "sent@test.com"),
            (3, "failed@test.com"),
            (4, "stopped@test.com"),
        ] {
            insert_request_raw(&db, content_id, "all-status", email, status, None).await;
        }

        let (tx, _rx) = tokio::sync::mpsc::channel(100);
        let app = crate::app::app(AppState::new(db, tx));

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/v1/topics/all-status")
                    .method("GET")
                    .header("X-API-KEY", get_api_key())
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

        assert_eq!(parsed["request_counts"]["Created"], 1);
        assert_eq!(parsed["request_counts"]["Processed"], 1);
        assert_eq!(parsed["request_counts"]["Sent"], 1);
        assert_eq!(parsed["request_counts"]["Failed"], 1);
        assert_eq!(parsed["request_counts"]["Stopped"], 1);
    }

    #[tokio::test]
    async fn test_topic_isolation() {
        let db = setup_db().await;
        let content_id = insert_default_content(&db).await;

        // topic_a
        insert_request_with_id(&db, 1, content_id, "topic_a", "a@test.com", 2, None).await;

        // topic_b
        insert_request_with_id(&db, 2, content_id, "topic_b", "b@test.com", 0, None).await;

        // Results for topic_a
        EmailResult {
            id: None,
            request_id: 1,
            status: "Delivery".to_string(),
            raw: None,
        }
        .save(&db)
        .await
        .unwrap();

        let (tx, _rx) = tokio::sync::mpsc::channel(100);
        let app = crate::app::app(AppState::new(db.clone(), tx.clone()));

        // Get topic_a
        let response = app
            .oneshot(
                Request::builder()
                    .uri("/v1/topics/topic_a")
                    .method("GET")
                    .header("X-API-KEY", get_api_key())
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let parsed: serde_json::Value = serde_json::from_slice(&body).unwrap();

        assert_eq!(parsed["request_counts"]["Sent"], 1);
        assert!(parsed["request_counts"]["Created"].is_null());
        assert_eq!(parsed["result_counts"]["Delivery"], 1);

        // Get topic_b
        let app2 = crate::app::app(AppState::new(db, tx));
        let response2 = app2
            .oneshot(
                Request::builder()
                    .uri("/v1/topics/topic_b")
                    .method("GET")
                    .header("X-API-KEY", get_api_key())
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        let body2 = axum::body::to_bytes(response2.into_body(), usize::MAX)
            .await
            .unwrap();
        let parsed2: serde_json::Value = serde_json::from_slice(&body2).unwrap();

        assert_eq!(parsed2["request_counts"]["Created"], 1);
        assert!(parsed2["request_counts"]["Sent"].is_null());
        assert!(parsed2["result_counts"].as_object().unwrap().is_empty());
    }

    #[tokio::test]
    async fn test_stop_already_stopped() {
        let db = setup_db().await;
        let content_id = insert_default_content(&db).await;

        // Already stopped (status = 4)
        insert_request_raw(&db, content_id, "already-stopped", "a@test.com", 4, None).await;

        let (tx, _rx) = tokio::sync::mpsc::channel(100);
        let app = crate::app::app(AppState::new(db.clone(), tx));

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/v1/topics/already-stopped")
                    .method("DELETE")
                    .header("X-API-KEY", get_api_key())
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);

        // Should still be stopped (no change)
        let row: (i32,) =
            sqlx::query_as("SELECT status FROM email_requests WHERE topic_id = 'already-stopped'")
                .fetch_one(&db)
                .await
                .unwrap();

        assert_eq!(row.0, EmailMessageStatus::Stopped as i32);
    }

    #[tokio::test]
    async fn test_topic_with_special_characters() {
        let db = setup_db().await;
        let content_id = insert_default_content(&db).await;

        // Topic with special characters
        insert_request_raw(
            &db,
            content_id,
            "topic-with-dashes_and_underscores",
            "a@test.com",
            0,
            None,
        )
        .await;

        let (tx, _rx) = tokio::sync::mpsc::channel(100);
        let app = crate::app::app(AppState::new(db, tx));

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/v1/topics/topic-with-dashes_and_underscores")
                    .method("GET")
                    .header("X-API-KEY", get_api_key())
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

        assert_eq!(parsed["request_counts"]["Created"], 1);
    }

    #[tokio::test]
    async fn test_large_topic() {
        let db = setup_db().await;
        let content_id = insert_default_content(&db).await;

        // Insert 100 requests
        for i in 1..=100 {
            let status = if i % 2 == 0 { 2 } else { 0 };
            insert_request_raw(
                &db,
                content_id,
                "large-topic",
                &format!("user{i}@test.com"),
                status,
                None,
            )
            .await;
        }

        let (tx, _rx) = tokio::sync::mpsc::channel(100);
        let app = crate::app::app(AppState::new(db, tx));

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/v1/topics/large-topic")
                    .method("GET")
                    .header("X-API-KEY", get_api_key())
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

        assert_eq!(parsed["request_counts"]["Created"], 50);
        assert_eq!(parsed["request_counts"]["Sent"], 50);
    }
}
