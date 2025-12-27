#[cfg(test)]
mod tests {
    use crate::handlers::message_handlers::{CreateMessageRequest, Message};
    use crate::state::AppState;
    use crate::tests::helpers::{get_api_key, setup_db};
    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use tower::ServiceExt;

    #[tokio::test]
    async fn test_create_message_success() {
        let db = setup_db().await;
        let (tx, _rx) = tokio::sync::mpsc::channel(100);
        let app = crate::app::app(AppState::new(db.clone(), tx));

        let payload = serde_json::json!({
            "messages": [{
                "topic_id": "test",
                "emails": ["user@test.com"],
                "subject": "Hello",
                "content": "<p>Test</p>"
            }]
        });

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/v1/messages")
                    .method("POST")
                    .header("Content-Type", "application/json")
                    .header("X-API-KEY", get_api_key())
                    .body(Body::from(serde_json::to_string(&payload).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_create_message_multiple_emails() {
        let db = setup_db().await;
        let (tx, _rx) = tokio::sync::mpsc::channel(100);
        let app = crate::app::app(AppState::new(db.clone(), tx));

        let payload = serde_json::json!({
            "messages": [{
                "topic_id": "bulk",
                "emails": ["a@test.com", "b@test.com", "c@test.com"],
                "subject": "Bulk",
                "content": "Test"
            }]
        });

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/v1/messages")
                    .method("POST")
                    .header("Content-Type", "application/json")
                    .header("X-API-KEY", get_api_key())
                    .body(Body::from(serde_json::to_string(&payload).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);

        let count: (i32,) = sqlx::query_as("SELECT COUNT(*) FROM email_requests")
            .fetch_one(&db)
            .await
            .unwrap();
        assert_eq!(count.0, 3);
    }

    #[tokio::test]
    async fn test_create_message_scheduled() {
        let db = setup_db().await;
        let (tx, _rx) = tokio::sync::mpsc::channel(100);
        let app = crate::app::app(AppState::new(db.clone(), tx));

        let payload = serde_json::json!({
            "messages": [{
                "topic_id": "scheduled",
                "emails": ["user@test.com"],
                "subject": "Scheduled",
                "content": "Test"
            }],
            "scheduled_at": "2025-01-01 10:00:00"
        });

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/v1/messages")
                    .method("POST")
                    .header("Content-Type", "application/json")
                    .header("X-API-KEY", get_api_key())
                    .body(Body::from(serde_json::to_string(&payload).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);

        let row: (i32,) =
            sqlx::query_as("SELECT status FROM email_requests WHERE topic_id = 'scheduled'")
                .fetch_one(&db)
                .await
                .unwrap();
        assert_eq!(row.0, 0); // Created
    }

    #[tokio::test]
    async fn test_create_message_unauthorized() {
        let db = setup_db().await;
        let (tx, _rx) = tokio::sync::mpsc::channel(100);
        let app = crate::app::app(AppState::new(db, tx));

        let payload = serde_json::json!({
            "messages": [{ "emails": ["test@test.com"], "subject": "Test", "content": "Test" }]
        });

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/v1/messages")
                    .method("POST")
                    .header("Content-Type", "application/json")
                    .body(Body::from(serde_json::to_string(&payload).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn test_get_topic_success() {
        let db = setup_db().await;
        let (tx, _rx) = tokio::sync::mpsc::channel(100);
        let app = crate::app::app(AppState::new(db.clone(), tx));

        sqlx::query(
            "INSERT INTO email_requests (topic_id, email, subject, content, scheduled_at, status)
             VALUES ('test-topic', 'a@test.com', 'test', 'test', datetime('now'), 0)",
        )
        .execute(&db)
        .await
        .unwrap();

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/v1/topics/test-topic")
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
    async fn test_stop_topic_success() {
        let db = setup_db().await;
        let (tx, _rx) = tokio::sync::mpsc::channel(100);
        let app = crate::app::app(AppState::new(db.clone(), tx));

        sqlx::query(
            "INSERT INTO email_requests (topic_id, email, subject, content, scheduled_at, status)
             VALUES ('stop-topic', 'a@test.com', 'test', 'test', datetime('now'), 0)",
        )
        .execute(&db)
        .await
        .unwrap();

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/v1/topics/stop-topic")
                    .method("DELETE")
                    .header("X-API-KEY", get_api_key())
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);

        let row: (i32,) =
            sqlx::query_as("SELECT status FROM email_requests WHERE topic_id = 'stop-topic'")
                .fetch_one(&db)
                .await
                .unwrap();
        assert_eq!(row.0, 4); // Stopped
    }

    #[tokio::test]
    async fn test_open_event_returns_image() {
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
        assert_eq!(response.headers().get("Content-Type").unwrap(), "image/png");
    }

    #[tokio::test]
    async fn test_create_message_empty_array() {
        let db = setup_db().await;
        let (tx, _rx) = tokio::sync::mpsc::channel(100);
        let app = crate::app::app(AppState::new(db, tx));

        let payload = serde_json::json!({ "messages": [] });

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/v1/messages")
                    .method("POST")
                    .header("Content-Type", "application/json")
                    .header("X-API-KEY", get_api_key())
                    .body(Body::from(serde_json::to_string(&payload).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn test_immediate_sends_to_channel() {
        let db = setup_db().await;
        let (tx, mut rx) = tokio::sync::mpsc::channel(100);
        let app = crate::app::app(AppState::new(db.clone(), tx));

        let payload = serde_json::json!({
            "messages": [{
                "topic_id": "immediate",
                "emails": ["user@test.com"],
                "subject": "Immediate",
                "content": "Test"
            }]
        });

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/v1/messages")
                    .method("POST")
                    .header("Content-Type", "application/json")
                    .header("X-API-KEY", get_api_key())
                    .body(Body::from(serde_json::to_string(&payload).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        assert!(rx.try_recv().is_ok());

        let row: (i32,) =
            sqlx::query_as("SELECT status FROM email_requests WHERE topic_id = 'immediate'")
                .fetch_one(&db)
                .await
                .unwrap();
        assert_eq!(row.0, 1); // Processed
    }

    #[tokio::test]
    async fn test_scheduled_not_sent_to_channel() {
        let db = setup_db().await;
        let (tx, mut rx) = tokio::sync::mpsc::channel(100);
        let app = crate::app::app(AppState::new(db.clone(), tx));

        let payload = serde_json::json!({
            "messages": [{
                "topic_id": "scheduled",
                "emails": ["user@test.com"],
                "subject": "Scheduled",
                "content": "Test"
            }],
            "scheduled_at": "2030-01-01 10:00:00"
        });

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/v1/messages")
                    .method("POST")
                    .header("Content-Type", "application/json")
                    .header("X-API-KEY", get_api_key())
                    .body(Body::from(serde_json::to_string(&payload).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        assert!(rx.try_recv().is_err());

        let row: (i32,) =
            sqlx::query_as("SELECT status FROM email_requests WHERE topic_id = 'scheduled'")
                .fetch_one(&db)
                .await
                .unwrap();
        assert_eq!(row.0, 0); // Created
    }

    #[test]
    fn test_message_deserialize() {
        let json = r#"{
            "topic_id": "test",
            "emails": ["a@test.com", "b@test.com"],
            "subject": "Hello",
            "content": "<p>World</p>"
        }"#;

        let msg: Message = serde_json::from_str(json).unwrap();
        assert_eq!(msg.topic_id, Some("test".to_string()));
        assert_eq!(msg.emails.len(), 2);
        assert_eq!(msg.subject, "Hello");
    }

    #[test]
    fn test_create_message_request_deserialize() {
        let json = r#"{
            "messages": [{
                "emails": ["test@test.com"],
                "subject": "Test",
                "content": "Content"
            }],
            "scheduled_at": "2025-01-01 00:00:00"
        }"#;

        let req: CreateMessageRequest = serde_json::from_str(json).unwrap();
        assert_eq!(req.messages.len(), 1);
        assert_eq!(req.scheduled_at, Some("2025-01-01 00:00:00".to_string()));
    }

    #[tokio::test]
    async fn test_create_message_exceeds_max_emails() {
        let db = setup_db().await;
        let (tx, _rx) = tokio::sync::mpsc::channel(100);
        let app = crate::app::app(AppState::new(db, tx));

        // 10,001 emails (exceeds MAX_EMAILS_PER_REQUEST)
        let emails: Vec<String> = (0..10_001).map(|i| format!("user{i}@test.com")).collect();

        let payload = serde_json::json!({
            "messages": [{
                "topic_id": "too_many",
                "emails": emails,
                "subject": "Test",
                "content": "Test"
            }]
        });

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/v1/messages")
                    .method("POST")
                    .header("Content-Type", "application/json")
                    .header("X-API-KEY", get_api_key())
                    .body(Body::from(serde_json::to_string(&payload).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::BAD_REQUEST);

        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let parsed: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert!(parsed["error"].as_str().unwrap().contains("10000"));
    }

    #[tokio::test]
    async fn test_create_message_multiple_messages() {
        let db = setup_db().await;
        let (tx, _rx) = tokio::sync::mpsc::channel(100);
        let app = crate::app::app(AppState::new(db.clone(), tx));

        let payload = serde_json::json!({
            "messages": [
                {
                    "topic_id": "topic_a",
                    "emails": ["a@test.com"],
                    "subject": "Subject A",
                    "content": "Content A"
                },
                {
                    "topic_id": "topic_b",
                    "emails": ["b@test.com", "c@test.com"],
                    "subject": "Subject B",
                    "content": "Content B"
                }
            ]
        });

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/v1/messages")
                    .method("POST")
                    .header("Content-Type", "application/json")
                    .header("X-API-KEY", get_api_key())
                    .body(Body::from(serde_json::to_string(&payload).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);

        let count: (i32,) = sqlx::query_as("SELECT COUNT(*) FROM email_requests")
            .fetch_one(&db)
            .await
            .unwrap();
        assert_eq!(count.0, 3);

        let topic_a_count: (i32,) =
            sqlx::query_as("SELECT COUNT(*) FROM email_requests WHERE topic_id = 'topic_a'")
                .fetch_one(&db)
                .await
                .unwrap();
        assert_eq!(topic_a_count.0, 1);

        let topic_b_count: (i32,) =
            sqlx::query_as("SELECT COUNT(*) FROM email_requests WHERE topic_id = 'topic_b'")
                .fetch_one(&db)
                .await
                .unwrap();
        assert_eq!(topic_b_count.0, 2);
    }

    #[tokio::test]
    async fn test_create_message_empty_scheduled_at() {
        let db = setup_db().await;
        let (tx, mut rx) = tokio::sync::mpsc::channel(100);
        let app = crate::app::app(AppState::new(db.clone(), tx));

        let payload = serde_json::json!({
            "messages": [{
                "topic_id": "empty_sched",
                "emails": ["user@test.com"],
                "subject": "Test",
                "content": "Test"
            }],
            "scheduled_at": ""
        });

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/v1/messages")
                    .method("POST")
                    .header("Content-Type", "application/json")
                    .header("X-API-KEY", get_api_key())
                    .body(Body::from(serde_json::to_string(&payload).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        // Empty scheduled_at should be treated as immediate
        assert!(rx.try_recv().is_ok());

        let row: (i32,) =
            sqlx::query_as("SELECT status FROM email_requests WHERE topic_id = 'empty_sched'")
                .fetch_one(&db)
                .await
                .unwrap();
        assert_eq!(row.0, 1); // Processed
    }

    #[tokio::test]
    async fn test_get_topic_returns_counts() {
        let db = setup_db().await;
        let (tx, _rx) = tokio::sync::mpsc::channel(100);
        let app = crate::app::app(AppState::new(db.clone(), tx));

        sqlx::query(
            "INSERT INTO email_requests (topic_id, email, subject, content, scheduled_at, status)
             VALUES ('count-topic', 'a@test.com', 'test', 'test', datetime('now'), 2)",
        )
        .execute(&db)
        .await
        .unwrap();

        sqlx::query(
            "INSERT INTO email_requests (topic_id, email, subject, content, scheduled_at, status)
             VALUES ('count-topic', 'b@test.com', 'test', 'test', datetime('now'), 0)",
        )
        .execute(&db)
        .await
        .unwrap();

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/v1/topics/count-topic")
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
        assert_eq!(parsed["request_counts"]["Created"], 1);
    }

    #[tokio::test]
    async fn test_get_topic_nonexistent() {
        let db = setup_db().await;
        let (tx, _rx) = tokio::sync::mpsc::channel(100);
        let app = crate::app::app(AppState::new(db, tx));

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/v1/topics/nonexistent-topic")
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
    }

    #[tokio::test]
    async fn test_stop_topic_only_stops_created() {
        let db = setup_db().await;
        let (tx, _rx) = tokio::sync::mpsc::channel(100);
        let app = crate::app::app(AppState::new(db.clone(), tx));

        // Created (should be stopped)
        sqlx::query(
            "INSERT INTO email_requests (topic_id, email, subject, content, scheduled_at, status)
             VALUES ('stop-test', 'a@test.com', 'test', 'test', datetime('now'), 0)",
        )
        .execute(&db)
        .await
        .unwrap();

        // Sent (should NOT be stopped)
        sqlx::query(
            "INSERT INTO email_requests (topic_id, email, subject, content, scheduled_at, status)
             VALUES ('stop-test', 'b@test.com', 'test', 'test', datetime('now'), 2)",
        )
        .execute(&db)
        .await
        .unwrap();

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/v1/topics/stop-test")
                    .method("DELETE")
                    .header("X-API-KEY", get_api_key())
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);

        let stopped: (i32,) = sqlx::query_as(
            "SELECT COUNT(*) FROM email_requests WHERE topic_id = 'stop-test' AND status = 4",
        )
        .fetch_one(&db)
        .await
        .unwrap();
        assert_eq!(stopped.0, 1);

        let sent: (i32,) = sqlx::query_as(
            "SELECT COUNT(*) FROM email_requests WHERE topic_id = 'stop-test' AND status = 2",
        )
        .fetch_one(&db)
        .await
        .unwrap();
        assert_eq!(sent.0, 1);
    }

    #[test]
    fn test_message_without_topic_id() {
        let json = r#"{
            "emails": ["a@test.com"],
            "subject": "Hello",
            "content": "<p>World</p>"
        }"#;

        let msg: Message = serde_json::from_str(json).unwrap();
        assert!(msg.topic_id.is_none());
        assert_eq!(msg.emails.len(), 1);
    }

    #[tokio::test]
    async fn test_create_message_response_fields() {
        let db = setup_db().await;
        let (tx, _rx) = tokio::sync::mpsc::channel(100);
        let app = crate::app::app(AppState::new(db, tx));

        let payload = serde_json::json!({
            "messages": [{
                "topic_id": "response_test",
                "emails": ["a@test.com", "b@test.com"],
                "subject": "Test",
                "content": "Test"
            }]
        });

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/v1/messages")
                    .method("POST")
                    .header("Content-Type", "application/json")
                    .header("X-API-KEY", get_api_key())
                    .body(Body::from(serde_json::to_string(&payload).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);

        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let parsed: serde_json::Value = serde_json::from_slice(&body).unwrap();

        assert_eq!(parsed["total"], 2);
        assert_eq!(parsed["success"], 2);
        assert_eq!(parsed["errors"], 0);
        assert_eq!(parsed["scheduled"], false);
        assert!(parsed["duration_ms"].as_u64().is_some());
    }

    #[tokio::test]
    async fn test_create_message_scheduled_response() {
        let db = setup_db().await;
        let (tx, _rx) = tokio::sync::mpsc::channel(100);
        let app = crate::app::app(AppState::new(db, tx));

        let payload = serde_json::json!({
            "messages": [{
                "topic_id": "sched_response",
                "emails": ["user@test.com"],
                "subject": "Test",
                "content": "Test"
            }],
            "scheduled_at": "2030-01-01 00:00:00"
        });

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/v1/messages")
                    .method("POST")
                    .header("Content-Type", "application/json")
                    .header("X-API-KEY", get_api_key())
                    .body(Body::from(serde_json::to_string(&payload).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();

        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let parsed: serde_json::Value = serde_json::from_slice(&body).unwrap();

        assert_eq!(parsed["scheduled"], true);
    }
}
