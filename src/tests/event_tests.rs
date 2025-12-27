#[cfg(test)]
mod tests {
    use crate::models::result::EmailResult;
    use crate::state::AppState;
    use crate::tests::helpers::{get_api_key, setup_db};
    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use sqlx::Row;
    use tower::ServiceExt;

    #[tokio::test]
    async fn test_open_returns_png() {
        let db = setup_db().await;
        let (tx, _) = tokio::sync::mpsc::channel(1);
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
    async fn test_open_creates_result() {
        let db = setup_db().await;

        sqlx::query(
            "INSERT INTO email_requests (id, topic_id, email, subject, content, scheduled_at)
             VALUES (1, 'topic', 'test@test.com', 'subject', 'content', datetime('now'))",
        )
        .execute(&db)
        .await
        .unwrap();

        let (tx, _) = tokio::sync::mpsc::channel(1);
        let app = crate::app::app(AppState::new(db.clone(), tx));

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/v1/events/open?request_id=1")
                    .method("GET")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);

        let result = sqlx::query("SELECT * FROM email_results WHERE request_id = 1")
            .fetch_one(&db)
            .await
            .unwrap();

        assert_eq!(result.get::<i64, _>("request_id"), 1);
        assert_eq!(result.get::<String, _>("status"), "Open");
    }

    #[tokio::test]
    async fn test_open_with_invalid_request_id() {
        let db = setup_db().await;
        let (tx, _) = tokio::sync::mpsc::channel(1);
        let app = crate::app::app(AppState::new(db, tx));

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/v1/events/open?request_id=999")
                    .method("GET")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_sent_count_unauthorized() {
        let db = setup_db().await;
        let (tx, _) = tokio::sync::mpsc::channel(1);
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
    async fn test_sent_count_returns_count() {
        let db = setup_db().await;

        sqlx::query(
            "INSERT INTO email_requests (topic_id, email, subject, content, status, scheduled_at)
             VALUES ('topic', 'test@test.com', 'subject', 'content', 2, datetime('now'))",
        )
        .execute(&db)
        .await
        .unwrap();

        let (tx, _) = tokio::sync::mpsc::channel(1);
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

        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let parsed: crate::handlers::event_handlers::SentCountResponse =
            serde_json::from_slice(&body).unwrap();

        assert_eq!(parsed.count, 1);
    }

    #[tokio::test]
    async fn test_email_result_save() {
        let db = setup_db().await;

        sqlx::query(
            "INSERT INTO email_requests (id, topic_id, email, subject, content, scheduled_at)
             VALUES (1, 'topic', 'test@test.com', 'subject', 'content', datetime('now'))",
        )
        .execute(&db)
        .await
        .unwrap();

        let result = EmailResult {
            id: None,
            request_id: 1,
            status: "Delivered".to_string(),
            raw: Some("raw data".to_string()),
        };

        let saved = result.save(&db).await.unwrap();
        assert!(saved.id.is_some());
    }

    #[tokio::test]
    async fn test_sns_without_header() {
        let db = setup_db().await;
        let (tx, _) = tokio::sync::mpsc::channel(1);
        let app = crate::app::app(AppState::new(db, tx));

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/v1/events/results")
                    .method("POST")
                    .header("Content-Type", "application/json")
                    .body(Body::from(r#"{"Message": "test"}"#))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn test_sns_subscription_confirmation() {
        let db = setup_db().await;
        let (tx, _) = tokio::sync::mpsc::channel(1);
        let app = crate::app::app(AppState::new(db, tx));

        let payload = serde_json::json!({
            "Type": "SubscriptionConfirmation",
            "SubscribeURL": "https://sns.amazonaws.com/confirm?token=abc123"
        });

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/v1/events/results")
                    .method("POST")
                    .header("Content-Type", "application/json")
                    .header("x-amz-sns-message-type", "SubscriptionConfirmation")
                    .body(Body::from(serde_json::to_string(&payload).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_sns_delivery_notification() {
        let db = setup_db().await;

        sqlx::query(
            "INSERT INTO email_requests (id, topic_id, email, subject, content, scheduled_at, message_id)
             VALUES (1, 'topic', 'test@test.com', 'subject', 'content', datetime('now'), 'ses-msg-123')",
        )
        .execute(&db)
        .await
        .unwrap();

        let (tx, _) = tokio::sync::mpsc::channel(1);
        let app = crate::app::app(AppState::new(db.clone(), tx));

        let ses_notification = serde_json::json!({
            "notificationType": "Delivery",
            "mail": { "messageId": "ses-msg-123" },
            "delivery": { "timestamp": "2024-01-01T00:00:00.000Z", "recipients": ["test@test.com"] }
        });

        let sns_payload = serde_json::json!({
            "Type": "Notification",
            "MessageId": "sns-msg-456",
            "Message": serde_json::to_string(&ses_notification).unwrap()
        });

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/v1/events/results")
                    .method("POST")
                    .header("Content-Type", "application/json")
                    .header("x-amz-sns-message-type", "Notification")
                    .body(Body::from(serde_json::to_string(&sns_payload).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);

        let result = sqlx::query("SELECT status FROM email_results WHERE request_id = 1")
            .fetch_one(&db)
            .await
            .unwrap();

        assert_eq!(result.get::<String, _>("status"), "Delivery");
    }

    #[tokio::test]
    async fn test_sns_bounce_notification() {
        let db = setup_db().await;

        sqlx::query(
            "INSERT INTO email_requests (id, topic_id, email, subject, content, scheduled_at, message_id)
             VALUES (1, 'topic', 'bounce@test.com', 'subject', 'content', datetime('now'), 'ses-bounce-123')",
        )
        .execute(&db)
        .await
        .unwrap();

        let (tx, _) = tokio::sync::mpsc::channel(1);
        let app = crate::app::app(AppState::new(db.clone(), tx));

        let ses_notification = serde_json::json!({
            "notificationType": "Bounce",
            "mail": { "messageId": "ses-bounce-123" },
            "bounce": { "bounceType": "Permanent", "bounceSubType": "General" }
        });

        let sns_payload = serde_json::json!({
            "Type": "Notification",
            "MessageId": "sns-bounce-msg",
            "Message": serde_json::to_string(&ses_notification).unwrap()
        });

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/v1/events/results")
                    .method("POST")
                    .header("Content-Type", "application/json")
                    .header("x-amz-sns-message-type", "Notification")
                    .body(Body::from(serde_json::to_string(&sns_payload).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);

        let result = sqlx::query("SELECT status FROM email_results WHERE request_id = 1")
            .fetch_one(&db)
            .await
            .unwrap();

        assert_eq!(result.get::<String, _>("status"), "Bounce");
    }

    #[tokio::test]
    async fn test_sns_missing_message_id() {
        let db = setup_db().await;
        let (tx, _) = tokio::sync::mpsc::channel(1);
        let app = crate::app::app(AppState::new(db, tx));

        let ses_notification = serde_json::json!({
            "notificationType": "Delivery",
            "mail": {}
        });

        let sns_payload = serde_json::json!({
            "Type": "Notification",
            "MessageId": "sns-msg",
            "Message": serde_json::to_string(&ses_notification).unwrap()
        });

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/v1/events/results")
                    .method("POST")
                    .header("Content-Type", "application/json")
                    .header("x-amz-sns-message-type", "Notification")
                    .body(Body::from(serde_json::to_string(&sns_payload).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn test_sns_invalid_json() {
        let db = setup_db().await;
        let (tx, _) = tokio::sync::mpsc::channel(1);
        let app = crate::app::app(AppState::new(db, tx));

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/v1/events/results")
                    .method("POST")
                    .header("Content-Type", "application/json")
                    .header("x-amz-sns-message-type", "Notification")
                    .body(Body::from("invalid json"))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn test_sns_complaint_notification() {
        let db = setup_db().await;

        sqlx::query(
            "INSERT INTO email_requests (id, topic_id, email, subject, content, scheduled_at, message_id)
             VALUES (1, 'topic', 'complaint@test.com', 'subject', 'content', datetime('now'), 'ses-complaint-123')",
        )
        .execute(&db)
        .await
        .unwrap();

        let (tx, _) = tokio::sync::mpsc::channel(1);
        let app = crate::app::app(AppState::new(db.clone(), tx));

        let ses_notification = serde_json::json!({
            "notificationType": "Complaint",
            "mail": { "messageId": "ses-complaint-123" },
            "complaint": { "complaintFeedbackType": "abuse" }
        });

        let sns_payload = serde_json::json!({
            "Type": "Notification",
            "MessageId": "sns-complaint-msg",
            "Message": serde_json::to_string(&ses_notification).unwrap()
        });

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/v1/events/results")
                    .method("POST")
                    .header("Content-Type", "application/json")
                    .header("x-amz-sns-message-type", "Notification")
                    .body(Body::from(serde_json::to_string(&sns_payload).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);

        let result = sqlx::query("SELECT status FROM email_results WHERE request_id = 1")
            .fetch_one(&db)
            .await
            .unwrap();

        assert_eq!(result.get::<String, _>("status"), "Complaint");
    }

    #[tokio::test]
    async fn test_sent_count_with_hours_param() {
        let db = setup_db().await;

        sqlx::query(
            "INSERT INTO email_requests (topic_id, email, subject, content, status, scheduled_at, created_at)
             VALUES ('topic', 'test@test.com', 'subject', 'content', 2, datetime('now'), datetime('now'))",
        )
        .execute(&db)
        .await
        .unwrap();

        let (tx, _) = tokio::sync::mpsc::channel(1);
        let app = crate::app::app(AppState::new(db, tx));

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/v1/events/counts/sent?hours=1")
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
        let parsed: crate::handlers::event_handlers::SentCountResponse =
            serde_json::from_slice(&body).unwrap();

        assert_eq!(parsed.count, 1);
    }

    #[tokio::test]
    async fn test_sent_count_empty_result() {
        let db = setup_db().await;
        let (tx, _) = tokio::sync::mpsc::channel(1);
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

        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let parsed: crate::handlers::event_handlers::SentCountResponse =
            serde_json::from_slice(&body).unwrap();

        assert_eq!(parsed.count, 0);
    }

    #[tokio::test]
    async fn test_open_with_invalid_request_id_format() {
        let db = setup_db().await;
        let (tx, _) = tokio::sync::mpsc::channel(1);
        let app = crate::app::app(AppState::new(db.clone(), tx));

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/v1/events/open?request_id=not_a_number")
                    .method("GET")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        // Should still return OK with the tracking pixel
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response.headers().get("Content-Type").unwrap(), "image/png");

        // Should not create any result record
        let count: (i32,) = sqlx::query_as("SELECT COUNT(*) FROM email_results")
            .fetch_one(&db)
            .await
            .unwrap();
        assert_eq!(count.0, 0);
    }

    #[tokio::test]
    async fn test_open_multiple_times() {
        let db = setup_db().await;

        sqlx::query(
            "INSERT INTO email_requests (id, topic_id, email, subject, content, scheduled_at)
             VALUES (1, 'topic', 'test@test.com', 'subject', 'content', datetime('now'))",
        )
        .execute(&db)
        .await
        .unwrap();

        let (tx, _) = tokio::sync::mpsc::channel(1);

        // First open
        let app1 = crate::app::app(AppState::new(db.clone(), tx.clone()));
        app1.oneshot(
            Request::builder()
                .uri("/v1/events/open?request_id=1")
                .method("GET")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

        // Second open
        let app2 = crate::app::app(AppState::new(db.clone(), tx));
        app2.oneshot(
            Request::builder()
                .uri("/v1/events/open?request_id=1")
                .method("GET")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

        // Should have 2 open records
        let count: (i32,) = sqlx::query_as("SELECT COUNT(*) FROM email_results WHERE request_id = 1")
            .fetch_one(&db)
            .await
            .unwrap();
        assert_eq!(count.0, 2);
    }

    #[tokio::test]
    async fn test_sns_notification_for_nonexistent_request() {
        let db = setup_db().await;
        let (tx, _) = tokio::sync::mpsc::channel(1);
        let app = crate::app::app(AppState::new(db, tx));

        let ses_notification = serde_json::json!({
            "notificationType": "Delivery",
            "mail": { "messageId": "nonexistent-ses-msg-id" }
        });

        let sns_payload = serde_json::json!({
            "Type": "Notification",
            "MessageId": "sns-msg",
            "Message": serde_json::to_string(&ses_notification).unwrap()
        });

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/v1/events/results")
                    .method("POST")
                    .header("Content-Type", "application/json")
                    .header("x-amz-sns-message-type", "Notification")
                    .body(Body::from(serde_json::to_string(&sns_payload).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
    }

    #[tokio::test]
    async fn test_email_result_preserves_raw() {
        let db = setup_db().await;

        sqlx::query(
            "INSERT INTO email_requests (id, topic_id, email, subject, content, scheduled_at)
             VALUES (1, 'topic', 'test@test.com', 'subject', 'content', datetime('now'))",
        )
        .execute(&db)
        .await
        .unwrap();

        let raw_data = r#"{"detailed": "event data", "timestamp": "2024-01-01"}"#;
        let result = EmailResult {
            id: None,
            request_id: 1,
            status: "Delivered".to_string(),
            raw: Some(raw_data.to_string()),
        };

        let saved = result.save(&db).await.unwrap();
        assert!(saved.id.is_some());

        let row = sqlx::query("SELECT raw FROM email_results WHERE id = ?")
            .bind(saved.id)
            .fetch_one(&db)
            .await
            .unwrap();

        assert_eq!(row.get::<Option<String>, _>("raw"), Some(raw_data.to_string()));
    }
}
