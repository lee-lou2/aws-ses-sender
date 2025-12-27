#[cfg(test)]
mod tests {
    use crate::models::request::{EmailMessageStatus, EmailRequest};
    use crate::tests::helpers::setup_db;

    fn create_test_request() -> EmailRequest {
        EmailRequest {
            id: None,
            topic_id: Some("test_topic".to_string()),
            email: "test@example.com".to_string(),
            subject: "Test Subject".to_string(),
            content: "<p>Test Content</p>".to_string(),
            scheduled_at: None,
            status: EmailMessageStatus::Created as i32,
            error: None,
            message_id: None,
        }
    }

    #[tokio::test]
    async fn test_save_returns_id() {
        let db = setup_db().await;
        let saved = create_test_request().save(&db).await.unwrap();

        assert!(saved.id.is_some());
        assert_eq!(saved.id, Some(1));
    }

    #[tokio::test]
    async fn test_save_preserves_fields() {
        let db = setup_db().await;
        let saved = create_test_request().save(&db).await.unwrap();

        assert_eq!(saved.topic_id, Some("test_topic".to_string()));
        assert_eq!(saved.email, "test@example.com");
        assert_eq!(saved.subject, "Test Subject");
        assert_eq!(saved.content, "<p>Test Content</p>");
    }

    #[tokio::test]
    async fn test_save_increments_id() {
        let db = setup_db().await;

        let saved1 = create_test_request().save(&db).await.unwrap();
        let saved2 = create_test_request().save(&db).await.unwrap();
        let saved3 = create_test_request().save(&db).await.unwrap();

        assert_eq!(saved1.id, Some(1));
        assert_eq!(saved2.id, Some(2));
        assert_eq!(saved3.id, Some(3));
    }

    #[tokio::test]
    async fn test_update_status() {
        let db = setup_db().await;
        let mut request = create_test_request().save(&db).await.unwrap();

        request.status = EmailMessageStatus::Sent as i32;
        request.update(&db).await.unwrap();

        let row: (i32,) = sqlx::query_as("SELECT status FROM email_requests WHERE id = ?")
            .bind(request.id)
            .fetch_one(&db)
            .await
            .unwrap();

        assert_eq!(row.0, EmailMessageStatus::Sent as i32);
    }

    #[tokio::test]
    async fn test_update_message_id() {
        let db = setup_db().await;
        let mut request = create_test_request().save(&db).await.unwrap();

        request.message_id = Some("ses-message-123".to_string());
        request.update(&db).await.unwrap();

        let row: (Option<String>,) =
            sqlx::query_as("SELECT message_id FROM email_requests WHERE id = ?")
                .bind(request.id)
                .fetch_one(&db)
                .await
                .unwrap();

        assert_eq!(row.0, Some("ses-message-123".to_string()));
    }

    #[tokio::test]
    async fn test_sent_count_empty() {
        let db = setup_db().await;
        let count = EmailRequest::sent_count(&db, 24).await.unwrap();
        assert_eq!(count, 0);
    }

    #[tokio::test]
    async fn test_sent_count_with_sent_emails() {
        let db = setup_db().await;

        let mut req1 = create_test_request().save(&db).await.unwrap();
        req1.status = EmailMessageStatus::Sent as i32;
        req1.update(&db).await.unwrap();

        let mut req2 = create_test_request().save(&db).await.unwrap();
        req2.status = EmailMessageStatus::Sent as i32;
        req2.update(&db).await.unwrap();

        create_test_request().save(&db).await.unwrap();

        let count = EmailRequest::sent_count(&db, 24).await.unwrap();
        assert_eq!(count, 2);
    }

    #[tokio::test]
    async fn test_stop_topic_updates_created_only() {
        let db = setup_db().await;

        create_test_request().save(&db).await.unwrap();
        create_test_request().save(&db).await.unwrap();

        let mut processed = create_test_request().save(&db).await.unwrap();
        processed.status = EmailMessageStatus::Processed as i32;
        processed.update(&db).await.unwrap();

        EmailRequest::stop_topic(&db, "test_topic").await.unwrap();

        let stopped: (i32,) =
            sqlx::query_as("SELECT COUNT(*) FROM email_requests WHERE status = ?")
                .bind(EmailMessageStatus::Stopped as i32)
                .fetch_one(&db)
                .await
                .unwrap();

        assert_eq!(stopped.0, 2);
    }

    #[tokio::test]
    async fn test_get_request_counts_grouped() {
        let db = setup_db().await;

        create_test_request().save(&db).await.unwrap();
        create_test_request().save(&db).await.unwrap();

        let mut sent = create_test_request().save(&db).await.unwrap();
        sent.status = EmailMessageStatus::Sent as i32;
        sent.update(&db).await.unwrap();

        let counts = EmailRequest::get_request_counts_by_topic_id(&db, "test_topic")
            .await
            .unwrap();

        assert_eq!(counts.get("Created"), Some(&2));
        assert_eq!(counts.get("Sent"), Some(&1));
    }

    #[tokio::test]
    async fn test_get_request_id_by_message_id() {
        let db = setup_db().await;

        let mut req = create_test_request().save(&db).await.unwrap();
        req.message_id = Some("ses-msg-abc123".to_string());
        req.update(&db).await.unwrap();

        let found_id = EmailRequest::get_request_id_by_message_id(&db, "ses-msg-abc123")
            .await
            .unwrap();

        assert_eq!(found_id, req.id.unwrap());
    }

    #[tokio::test]
    async fn test_save_batch_empty() {
        let db = setup_db().await;
        let result = EmailRequest::save_batch(vec![], &db).await.unwrap();
        assert!(result.is_empty());
    }

    #[tokio::test]
    async fn test_save_batch_single() {
        let db = setup_db().await;
        let saved = EmailRequest::save_batch(vec![create_test_request()], &db)
            .await
            .unwrap();

        assert_eq!(saved.len(), 1);
        assert_eq!(saved[0].id, Some(1));
    }

    #[tokio::test]
    async fn test_save_batch_multiple() {
        let db = setup_db().await;
        let requests: Vec<EmailRequest> = (0..5)
            .map(|i| EmailRequest {
                id: None,
                topic_id: Some("batch_topic".to_string()),
                email: format!("user{i}@example.com"),
                subject: format!("Subject {i}"),
                content: format!("<p>Content {i}</p>"),
                scheduled_at: None,
                status: EmailMessageStatus::Created as i32,
                error: None,
                message_id: None,
            })
            .collect();

        let saved = EmailRequest::save_batch(requests, &db).await.unwrap();

        assert_eq!(saved.len(), 5);

        for (i, req) in saved.iter().enumerate() {
            assert_eq!(req.id, Some((i + 1) as i32));
        }

        let count: (i32,) = sqlx::query_as("SELECT COUNT(*) FROM email_requests")
            .fetch_one(&db)
            .await
            .unwrap();
        assert_eq!(count.0, 5);
    }

    #[tokio::test]
    async fn test_save_batch_large() {
        let db = setup_db().await;

        let requests: Vec<EmailRequest> = (0..250)
            .map(|i| EmailRequest {
                id: None,
                topic_id: Some("large_batch".to_string()),
                email: format!("user{i}@example.com"),
                subject: "Test".to_string(),
                content: "<p>Test</p>".to_string(),
                scheduled_at: None,
                status: EmailMessageStatus::Created as i32,
                error: None,
                message_id: None,
            })
            .collect();

        let saved = EmailRequest::save_batch(requests, &db).await.unwrap();

        assert_eq!(saved.len(), 250);

        let count: (i32,) = sqlx::query_as("SELECT COUNT(*) FROM email_requests")
            .fetch_one(&db)
            .await
            .unwrap();
        assert_eq!(count.0, 250);
    }

    #[tokio::test]
    async fn test_get_request_id_by_message_id_not_found() {
        let db = setup_db().await;

        let result = EmailRequest::get_request_id_by_message_id(&db, "nonexistent-message-id").await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_update_error_field() {
        let db = setup_db().await;
        let mut request = create_test_request().save(&db).await.unwrap();

        request.status = EmailMessageStatus::Failed as i32;
        request.error = Some("SES rate limit exceeded".to_string());
        request.update(&db).await.unwrap();

        let row: (i32, Option<String>) =
            sqlx::query_as("SELECT status, error FROM email_requests WHERE id = ?")
                .bind(request.id)
                .fetch_one(&db)
                .await
                .unwrap();

        assert_eq!(row.0, EmailMessageStatus::Failed as i32);
        assert_eq!(row.1, Some("SES rate limit exceeded".to_string()));
    }

    #[tokio::test]
    async fn test_get_request_counts_nonexistent_topic() {
        let db = setup_db().await;

        let counts = EmailRequest::get_request_counts_by_topic_id(&db, "nonexistent_topic")
            .await
            .unwrap();

        assert!(counts.is_empty());
    }

    #[tokio::test]
    async fn test_stop_topic_no_matching_records() {
        let db = setup_db().await;

        // Insert requests with Processed status (not Created)
        let mut req = create_test_request().save(&db).await.unwrap();
        req.status = EmailMessageStatus::Processed as i32;
        req.update(&db).await.unwrap();

        EmailRequest::stop_topic(&db, "test_topic").await.unwrap();

        // Should still be Processed
        let row: (i32,) = sqlx::query_as("SELECT status FROM email_requests WHERE id = ?")
            .bind(req.id)
            .fetch_one(&db)
            .await
            .unwrap();

        assert_eq!(row.0, EmailMessageStatus::Processed as i32);
    }

    #[tokio::test]
    async fn test_get_request_counts_all_statuses() {
        let db = setup_db().await;

        // Created
        create_test_request().save(&db).await.unwrap();

        // Processed
        let mut processed = create_test_request().save(&db).await.unwrap();
        processed.status = EmailMessageStatus::Processed as i32;
        processed.update(&db).await.unwrap();

        // Sent
        let mut sent = create_test_request().save(&db).await.unwrap();
        sent.status = EmailMessageStatus::Sent as i32;
        sent.update(&db).await.unwrap();

        // Failed
        let mut failed = create_test_request().save(&db).await.unwrap();
        failed.status = EmailMessageStatus::Failed as i32;
        failed.update(&db).await.unwrap();

        // Stopped
        let mut stopped = create_test_request().save(&db).await.unwrap();
        stopped.status = EmailMessageStatus::Stopped as i32;
        stopped.update(&db).await.unwrap();

        let counts = EmailRequest::get_request_counts_by_topic_id(&db, "test_topic")
            .await
            .unwrap();

        assert_eq!(counts.get("Created"), Some(&1));
        assert_eq!(counts.get("Processed"), Some(&1));
        assert_eq!(counts.get("Sent"), Some(&1));
        assert_eq!(counts.get("Failed"), Some(&1));
        assert_eq!(counts.get("Stopped"), Some(&1));
    }

    #[tokio::test]
    async fn test_multiple_topics_isolation() {
        let db = setup_db().await;

        // topic_a
        let req_a = EmailRequest {
            id: None,
            topic_id: Some("topic_a".to_string()),
            email: "a@test.com".to_string(),
            subject: "Test".to_string(),
            content: "Test".to_string(),
            scheduled_at: None,
            status: EmailMessageStatus::Created as i32,
            error: None,
            message_id: None,
        };
        req_a.save(&db).await.unwrap();

        // topic_b
        let req_b = EmailRequest {
            id: None,
            topic_id: Some("topic_b".to_string()),
            email: "b@test.com".to_string(),
            subject: "Test".to_string(),
            content: "Test".to_string(),
            scheduled_at: None,
            status: EmailMessageStatus::Created as i32,
            error: None,
            message_id: None,
        };
        req_b.save(&db).await.unwrap();

        EmailRequest::stop_topic(&db, "topic_a").await.unwrap();

        let counts_a = EmailRequest::get_request_counts_by_topic_id(&db, "topic_a")
            .await
            .unwrap();
        let counts_b = EmailRequest::get_request_counts_by_topic_id(&db, "topic_b")
            .await
            .unwrap();

        assert_eq!(counts_a.get("Stopped"), Some(&1));
        assert_eq!(counts_b.get("Created"), Some(&1));
    }
}
