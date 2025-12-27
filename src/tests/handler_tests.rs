#[cfg(test)]
mod tests {
    use crate::handlers::message_handlers::{CreateMessageRequest, Message};
    use crate::state::AppState;
    use crate::tests::helpers::{
        get_api_key, insert_default_content, insert_request_raw, setup_db,
    };
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

        // Verify content was saved
        let content_count: (i32,) = sqlx::query_as("SELECT COUNT(*) FROM email_contents")
            .fetch_one(&db)
            .await
            .unwrap();
        assert_eq!(content_count.0, 1);
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

        // 3 requests, but only 1 content
        let req_count: (i32,) = sqlx::query_as("SELECT COUNT(*) FROM email_requests")
            .fetch_one(&db)
            .await
            .unwrap();
        assert_eq!(req_count.0, 3);

        let content_count: (i32,) = sqlx::query_as("SELECT COUNT(*) FROM email_contents")
            .fetch_one(&db)
            .await
            .unwrap();
        assert_eq!(content_count.0, 1);
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
        let content_id = insert_default_content(&db).await;
        insert_request_raw(&db, content_id, "test-topic", "a@test.com", 0, None).await;

        let (tx, _rx) = tokio::sync::mpsc::channel(100);
        let app = crate::app::app(AppState::new(db, tx));

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
        let content_id = insert_default_content(&db).await;
        insert_request_raw(&db, content_id, "stop-topic", "a@test.com", 0, None).await;

        let (tx, _rx) = tokio::sync::mpsc::channel(100);
        let app = crate::app::app(AppState::new(db.clone(), tx));

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
    #[allow(clippy::similar_names)]
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

        // 3 requests, 2 contents (one per message)
        let req_count: (i32,) = sqlx::query_as("SELECT COUNT(*) FROM email_requests")
            .fetch_one(&db)
            .await
            .unwrap();
        assert_eq!(req_count.0, 3);

        let content_count: (i32,) = sqlx::query_as("SELECT COUNT(*) FROM email_contents")
            .fetch_one(&db)
            .await
            .unwrap();
        assert_eq!(content_count.0, 2);

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
        let content_id = insert_default_content(&db).await;

        // Sent
        insert_request_raw(&db, content_id, "count-topic", "a@test.com", 2, None).await;
        // Created
        insert_request_raw(&db, content_id, "count-topic", "b@test.com", 0, None).await;

        let (tx, _rx) = tokio::sync::mpsc::channel(100);
        let app = crate::app::app(AppState::new(db, tx));

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
        let content_id = insert_default_content(&db).await;

        // Created (should be stopped)
        insert_request_raw(&db, content_id, "stop-test", "a@test.com", 0, None).await;
        // Sent (should NOT be stopped)
        insert_request_raw(&db, content_id, "stop-test", "b@test.com", 2, None).await;

        let (tx, _rx) = tokio::sync::mpsc::channel(100);
        let app = crate::app::app(AppState::new(db.clone(), tx));

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

    #[tokio::test]
    async fn test_create_message_saves_content_id() {
        let db = setup_db().await;
        let (tx, _rx) = tokio::sync::mpsc::channel(100);
        let app = crate::app::app(AppState::new(db.clone(), tx));

        let payload = serde_json::json!({
            "messages": [{
                "topic_id": "content_id_test",
                "emails": ["user@test.com"],
                "subject": "Test Subject",
                "content": "Test Content"
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

        // Verify content_id is set in request
        let row: (i32,) = sqlx::query_as(
            "SELECT content_id FROM email_requests WHERE topic_id = 'content_id_test'",
        )
        .fetch_one(&db)
        .await
        .unwrap();

        assert!(row.0 > 0);

        // Verify content exists with that id
        let content: (String, String) =
            sqlx::query_as("SELECT subject, content FROM email_contents WHERE id = ?")
                .bind(row.0)
                .fetch_one(&db)
                .await
                .unwrap();

        assert_eq!(content.0, "Test Subject");
        assert_eq!(content.1, "Test Content");
    }

    #[tokio::test]
    async fn test_multiple_emails_share_same_content_id() {
        let db = setup_db().await;
        let (tx, _rx) = tokio::sync::mpsc::channel(100);
        let app = crate::app::app(AppState::new(db.clone(), tx));

        let payload = serde_json::json!({
            "messages": [{
                "topic_id": "shared_content",
                "emails": ["a@test.com", "b@test.com", "c@test.com"],
                "subject": "Shared Subject",
                "content": "Shared Content"
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

        // All requests should have the same content_id
        let rows: Vec<(i32,)> = sqlx::query_as(
            "SELECT DISTINCT content_id FROM email_requests WHERE topic_id = 'shared_content'",
        )
        .fetch_all(&db)
        .await
        .unwrap();

        assert_eq!(
            rows.len(),
            1,
            "All requests should share the same content_id"
        );

        // Verify only 1 content record
        let content_count: (i32,) = sqlx::query_as("SELECT COUNT(*) FROM email_contents")
            .fetch_one(&db)
            .await
            .unwrap();
        assert_eq!(content_count.0, 1);
    }

    #[tokio::test]
    async fn test_different_messages_have_different_content_ids() {
        let db = setup_db().await;
        let (tx, _rx) = tokio::sync::mpsc::channel(100);
        let app = crate::app::app(AppState::new(db.clone(), tx));

        let payload = serde_json::json!({
            "messages": [
                {
                    "topic_id": "topic_1",
                    "emails": ["a@test.com"],
                    "subject": "Subject 1",
                    "content": "Content 1"
                },
                {
                    "topic_id": "topic_2",
                    "emails": ["b@test.com"],
                    "subject": "Subject 2",
                    "content": "Content 2"
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

        // Each message should have its own content_id
        let content_ids: Vec<(i32,)> =
            sqlx::query_as("SELECT content_id FROM email_requests ORDER BY id")
                .fetch_all(&db)
                .await
                .unwrap();

        assert_eq!(content_ids.len(), 2);
        assert_ne!(content_ids[0].0, content_ids[1].0);

        // Verify 2 content records
        let content_count: (i32,) = sqlx::query_as("SELECT COUNT(*) FROM email_contents")
            .fetch_one(&db)
            .await
            .unwrap();
        assert_eq!(content_count.0, 2);
    }

    #[tokio::test]
    async fn test_request_content_join() {
        let db = setup_db().await;
        let (tx, _rx) = tokio::sync::mpsc::channel(100);
        let app = crate::app::app(AppState::new(db.clone(), tx));

        let payload = serde_json::json!({
            "messages": [{
                "topic_id": "join_test",
                "emails": ["user@test.com"],
                "subject": "Join Subject",
                "content": "<p>Join Content</p>"
            }]
        });

        app.oneshot(
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

        // Query with JOIN to get subject/content
        let row: (String, String, String) = sqlx::query_as(
            "SELECT r.email, c.subject, c.content
             FROM email_requests r
             JOIN email_contents c ON r.content_id = c.id
             WHERE r.topic_id = 'join_test'",
        )
        .fetch_one(&db)
        .await
        .unwrap();

        assert_eq!(row.0, "user@test.com");
        assert_eq!(row.1, "Join Subject");
        assert_eq!(row.2, "<p>Join Content</p>");
    }

    #[tokio::test]
    async fn test_create_message_with_special_characters() {
        let db = setup_db().await;
        let (tx, _rx) = tokio::sync::mpsc::channel(100);
        let app = crate::app::app(AppState::new(db.clone(), tx));

        let payload = serde_json::json!({
            "messages": [{
                "topic_id": "special_chars",
                "emails": ["user@test.com"],
                "subject": "Hello 'World' \"Test\" <>&",
                "content": "<html><body>한글 테스트 🎉 émojis</body></html>"
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

        let row: (String, String) = sqlx::query_as(
            "SELECT c.subject, c.content
             FROM email_requests r
             JOIN email_contents c ON r.content_id = c.id
             WHERE r.topic_id = 'special_chars'",
        )
        .fetch_one(&db)
        .await
        .unwrap();

        assert_eq!(row.0, "Hello 'World' \"Test\" <>&");
        assert_eq!(row.1, "<html><body>한글 테스트 🎉 émojis</body></html>");
    }

    #[tokio::test]
    async fn test_create_message_without_topic_id() {
        let db = setup_db().await;
        let (tx, _rx) = tokio::sync::mpsc::channel(100);
        let app = crate::app::app(AppState::new(db.clone(), tx));

        let payload = serde_json::json!({
            "messages": [{
                "emails": ["user@test.com"],
                "subject": "No Topic",
                "content": "Content"
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

        // topic_id should be empty string (default)
        let row: (String,) = sqlx::query_as("SELECT topic_id FROM email_requests LIMIT 1")
            .fetch_one(&db)
            .await
            .unwrap();

        assert_eq!(row.0, "");
    }

    #[tokio::test]
    async fn test_create_message_large_batch() {
        let db = setup_db().await;
        let (tx, _rx) = tokio::sync::mpsc::channel(10000);
        let app = crate::app::app(AppState::new(db.clone(), tx));

        let emails: Vec<String> = (0..500).map(|i| format!("user{i}@test.com")).collect();

        let payload = serde_json::json!({
            "messages": [{
                "topic_id": "large_batch",
                "emails": emails,
                "subject": "Large Batch",
                "content": "Content"
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

        assert_eq!(parsed["total"], 500);
        assert_eq!(parsed["success"], 500);

        // Verify all share same content_id
        let distinct_content_ids: Vec<(i32,)> = sqlx::query_as(
            "SELECT DISTINCT content_id FROM email_requests WHERE topic_id = 'large_batch'",
        )
        .fetch_all(&db)
        .await
        .unwrap();

        assert_eq!(distinct_content_ids.len(), 1);
    }
}
