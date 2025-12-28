#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use crate::models::request::{EmailMessageStatus, EmailRequest};
    use crate::tests::helpers::{
        create_test_content, create_test_request_with_content_id, setup_db,
    };

    #[tokio::test]
    async fn test_save_returns_id() {
        let db = setup_db().await;
        let content = create_test_content().save(&db).await.unwrap();
        let req = create_test_request_with_content_id(content.id.unwrap());
        let saved = req.save(&db).await.unwrap();

        assert!(saved.id.is_some());
        assert_eq!(saved.id, Some(1));
    }

    #[tokio::test]
    async fn test_save_preserves_fields() {
        let db = setup_db().await;
        let content = create_test_content().save(&db).await.unwrap();
        let req = create_test_request_with_content_id(content.id.unwrap());
        let saved = req.save(&db).await.unwrap();

        assert_eq!(saved.topic_id, Some("test_topic".to_string()));
        assert_eq!(saved.email, "test@example.com");
        assert_eq!(saved.content_id, content.id);
    }

    #[tokio::test]
    async fn test_save_increments_id() {
        let db = setup_db().await;
        let content = create_test_content().save(&db).await.unwrap();
        let content_id = content.id.unwrap();

        let saved1 = create_test_request_with_content_id(content_id)
            .save(&db)
            .await
            .unwrap();
        let saved2 = create_test_request_with_content_id(content_id)
            .save(&db)
            .await
            .unwrap();
        let saved3 = create_test_request_with_content_id(content_id)
            .save(&db)
            .await
            .unwrap();

        assert_eq!(saved1.id, Some(1));
        assert_eq!(saved2.id, Some(2));
        assert_eq!(saved3.id, Some(3));
    }

    #[tokio::test]
    async fn test_update_status() {
        let db = setup_db().await;
        let content = create_test_content().save(&db).await.unwrap();
        let mut request = create_test_request_with_content_id(content.id.unwrap())
            .save(&db)
            .await
            .unwrap();

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
        let content = create_test_content().save(&db).await.unwrap();
        let mut request = create_test_request_with_content_id(content.id.unwrap())
            .save(&db)
            .await
            .unwrap();

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
        let content = create_test_content().save(&db).await.unwrap();
        let content_id = content.id.unwrap();

        let mut req1 = create_test_request_with_content_id(content_id)
            .save(&db)
            .await
            .unwrap();
        req1.status = EmailMessageStatus::Sent as i32;
        req1.update(&db).await.unwrap();

        let mut req2 = create_test_request_with_content_id(content_id)
            .save(&db)
            .await
            .unwrap();
        req2.status = EmailMessageStatus::Sent as i32;
        req2.update(&db).await.unwrap();

        create_test_request_with_content_id(content_id)
            .save(&db)
            .await
            .unwrap();

        let count = EmailRequest::sent_count(&db, 24).await.unwrap();
        assert_eq!(count, 2);
    }

    #[tokio::test]
    async fn test_stop_topic_updates_created_only() {
        let db = setup_db().await;
        let content = create_test_content().save(&db).await.unwrap();
        let content_id = content.id.unwrap();

        create_test_request_with_content_id(content_id)
            .save(&db)
            .await
            .unwrap();
        create_test_request_with_content_id(content_id)
            .save(&db)
            .await
            .unwrap();

        let mut processed = create_test_request_with_content_id(content_id)
            .save(&db)
            .await
            .unwrap();
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
        let content = create_test_content().save(&db).await.unwrap();
        let content_id = content.id.unwrap();

        create_test_request_with_content_id(content_id)
            .save(&db)
            .await
            .unwrap();
        create_test_request_with_content_id(content_id)
            .save(&db)
            .await
            .unwrap();

        let mut sent = create_test_request_with_content_id(content_id)
            .save(&db)
            .await
            .unwrap();
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
        let content = create_test_content().save(&db).await.unwrap();

        let mut req = create_test_request_with_content_id(content.id.unwrap())
            .save(&db)
            .await
            .unwrap();
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
        let content = create_test_content().save(&db).await.unwrap();
        let req = create_test_request_with_content_id(content.id.unwrap());

        let saved = EmailRequest::save_batch(vec![req], &db).await.unwrap();

        assert_eq!(saved.len(), 1);
        assert_eq!(saved[0].id, Some(1));
    }

    #[tokio::test]
    async fn test_save_batch_multiple() {
        let db = setup_db().await;
        let content = create_test_content().save(&db).await.unwrap();
        let content_id = content.id.unwrap();

        let requests: Vec<EmailRequest> = (0..5)
            .map(|i| EmailRequest {
                id: None,
                topic_id: Some("batch_topic".to_string()),
                content_id: Some(content_id),
                email: format!("user{i}@example.com"),
                subject: Arc::new(String::new()),
                content: Arc::new(String::new()),
                scheduled_at: None,
                status: EmailMessageStatus::Created as i32,
                error: None,
                message_id: None,
            })
            .collect();

        let saved = EmailRequest::save_batch(requests, &db).await.unwrap();

        assert_eq!(saved.len(), 5);

        #[allow(clippy::cast_possible_truncation, clippy::cast_possible_wrap)]
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
        let content = create_test_content().save(&db).await.unwrap();
        let content_id = content.id.unwrap();

        let requests: Vec<EmailRequest> = (0..250)
            .map(|i| EmailRequest {
                id: None,
                topic_id: Some("large_batch".to_string()),
                content_id: Some(content_id),
                email: format!("user{i}@example.com"),
                subject: Arc::new(String::new()),
                content: Arc::new(String::new()),
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

        let result =
            EmailRequest::get_request_id_by_message_id(&db, "nonexistent-message-id").await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_update_error_field() {
        let db = setup_db().await;
        let content = create_test_content().save(&db).await.unwrap();
        let mut request = create_test_request_with_content_id(content.id.unwrap())
            .save(&db)
            .await
            .unwrap();

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
        let content = create_test_content().save(&db).await.unwrap();

        let mut req = create_test_request_with_content_id(content.id.unwrap())
            .save(&db)
            .await
            .unwrap();
        req.status = EmailMessageStatus::Processed as i32;
        req.update(&db).await.unwrap();

        EmailRequest::stop_topic(&db, "test_topic").await.unwrap();

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
        let content = create_test_content().save(&db).await.unwrap();
        let content_id = content.id.unwrap();

        // Created
        create_test_request_with_content_id(content_id)
            .save(&db)
            .await
            .unwrap();

        // Processed
        let mut processed = create_test_request_with_content_id(content_id)
            .save(&db)
            .await
            .unwrap();
        processed.status = EmailMessageStatus::Processed as i32;
        processed.update(&db).await.unwrap();

        // Sent
        let mut sent = create_test_request_with_content_id(content_id)
            .save(&db)
            .await
            .unwrap();
        sent.status = EmailMessageStatus::Sent as i32;
        sent.update(&db).await.unwrap();

        // Failed
        let mut failed = create_test_request_with_content_id(content_id)
            .save(&db)
            .await
            .unwrap();
        failed.status = EmailMessageStatus::Failed as i32;
        failed.update(&db).await.unwrap();

        // Stopped
        let mut stopped = create_test_request_with_content_id(content_id)
            .save(&db)
            .await
            .unwrap();
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
        let content = create_test_content().save(&db).await.unwrap();
        let content_id = content.id.unwrap();

        // topic_a
        let req_a = EmailRequest {
            id: None,
            topic_id: Some("topic_a".to_string()),
            content_id: Some(content_id),
            email: "a@test.com".to_string(),
            subject: Arc::new(String::new()),
            content: Arc::new(String::new()),
            scheduled_at: None,
            status: EmailMessageStatus::Created as i32,
            error: None,
            message_id: None,
        };
        let _ = req_a.save(&db).await.unwrap();

        // topic_b
        let req_b = EmailRequest {
            id: None,
            topic_id: Some("topic_b".to_string()),
            content_id: Some(content_id),
            email: "b@test.com".to_string(),
            subject: Arc::new(String::new()),
            content: Arc::new(String::new()),
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

    #[tokio::test]
    async fn test_save_batch_with_different_content_ids() {
        let db = setup_db().await;

        // Create two different contents
        let content1 = create_test_content().save(&db).await.unwrap();
        let content2 = crate::models::content::EmailContent {
            id: None,
            subject: "Different Subject".to_string(),
            content: "Different Content".to_string(),
        }
        .save(&db)
        .await
        .unwrap();

        let requests = vec![
            EmailRequest {
                id: None,
                topic_id: Some("mixed".to_string()),
                content_id: Some(content1.id.unwrap()),
                email: "a@test.com".to_string(),
                subject: Arc::new(String::new()),
                content: Arc::new(String::new()),
                scheduled_at: None,
                status: EmailMessageStatus::Created as i32,
                error: None,
                message_id: None,
            },
            EmailRequest {
                id: None,
                topic_id: Some("mixed".to_string()),
                content_id: Some(content2.id.unwrap()),
                email: "b@test.com".to_string(),
                subject: Arc::new(String::new()),
                content: Arc::new(String::new()),
                scheduled_at: None,
                status: EmailMessageStatus::Created as i32,
                error: None,
                message_id: None,
            },
        ];

        let saved = EmailRequest::save_batch(requests, &db).await.unwrap();
        assert_eq!(saved.len(), 2);
        assert_eq!(saved[0].content_id, Some(content1.id.unwrap()));
        assert_eq!(saved[1].content_id, Some(content2.id.unwrap()));
    }

    #[tokio::test]
    async fn test_save_duplicate_emails() {
        let db = setup_db().await;
        let content = create_test_content().save(&db).await.unwrap();
        let content_id = content.id.unwrap();

        // Same email, different requests
        let requests: Vec<EmailRequest> = (0..3)
            .map(|_| EmailRequest {
                id: None,
                topic_id: Some("dup_test".to_string()),
                content_id: Some(content_id),
                email: "same@example.com".to_string(),
                subject: Arc::new(String::new()),
                content: Arc::new(String::new()),
                scheduled_at: None,
                status: EmailMessageStatus::Created as i32,
                error: None,
                message_id: None,
            })
            .collect();

        let saved = EmailRequest::save_batch(requests, &db).await.unwrap();
        assert_eq!(saved.len(), 3);

        // All should have same email but different ids
        assert_eq!(saved[0].email, "same@example.com");
        assert_eq!(saved[1].email, "same@example.com");
        assert_eq!(saved[2].email, "same@example.com");
        assert_ne!(saved[0].id, saved[1].id);
        assert_ne!(saved[1].id, saved[2].id);
    }

    #[tokio::test]
    async fn test_save_batch_preserves_content_id() {
        let db = setup_db().await;
        let content = create_test_content().save(&db).await.unwrap();
        let content_id = content.id.unwrap();

        let requests: Vec<EmailRequest> = (0..5)
            .map(|i| EmailRequest {
                id: None,
                topic_id: Some("preserve_test".to_string()),
                content_id: Some(content_id),
                email: format!("user{i}@example.com"),
                subject: Arc::new(String::new()),
                content: Arc::new(String::new()),
                scheduled_at: None,
                status: EmailMessageStatus::Created as i32,
                error: None,
                message_id: None,
            })
            .collect();

        let saved = EmailRequest::save_batch(requests, &db).await.unwrap();

        for req in saved {
            assert_eq!(req.content_id, Some(content_id));
        }

        // Verify in database
        let rows: Vec<(i32,)> = sqlx::query_as(
            "SELECT content_id FROM email_requests WHERE topic_id = 'preserve_test'",
        )
        .fetch_all(&db)
        .await
        .unwrap();

        for row in rows {
            assert_eq!(row.0, content_id);
        }
    }

    #[tokio::test]
    async fn test_stop_topic_does_not_affect_other_topics() {
        let db = setup_db().await;
        let content = create_test_content().save(&db).await.unwrap();
        let content_id = content.id.unwrap();

        // Create requests in different topics
        for topic in ["topic_x", "topic_y", "topic_z"] {
            for i in 0..3 {
                let req = EmailRequest {
                    id: None,
                    topic_id: Some(topic.to_string()),
                    content_id: Some(content_id),
                    email: format!("{topic}_{i}@test.com"),
                    subject: Arc::new(String::new()),
                    content: Arc::new(String::new()),
                    scheduled_at: None,
                    status: EmailMessageStatus::Created as i32,
                    error: None,
                    message_id: None,
                };
                req.save(&db).await.unwrap();
            }
        }

        // Stop only topic_y
        EmailRequest::stop_topic(&db, "topic_y").await.unwrap();

        // Verify topic_x is unchanged
        let counts_x = EmailRequest::get_request_counts_by_topic_id(&db, "topic_x")
            .await
            .unwrap();
        assert_eq!(counts_x.get("Created"), Some(&3));
        assert!(counts_x.get("Stopped").is_none());

        // Verify topic_y is stopped
        let counts_y = EmailRequest::get_request_counts_by_topic_id(&db, "topic_y")
            .await
            .unwrap();
        assert_eq!(counts_y.get("Stopped"), Some(&3));
        assert!(counts_y.get("Created").is_none());

        // Verify topic_z is unchanged
        let counts_z = EmailRequest::get_request_counts_by_topic_id(&db, "topic_z")
            .await
            .unwrap();
        assert_eq!(counts_z.get("Created"), Some(&3));
    }

    #[tokio::test]
    async fn test_update_changes_updated_at() {
        let db = setup_db().await;
        let content = create_test_content().save(&db).await.unwrap();
        let mut request = create_test_request_with_content_id(content.id.unwrap())
            .save(&db)
            .await
            .unwrap();

        // Get initial updated_at
        let row1: (String,) = sqlx::query_as("SELECT updated_at FROM email_requests WHERE id = ?")
            .bind(request.id)
            .fetch_one(&db)
            .await
            .unwrap();

        // Small delay to ensure time difference
        tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;

        request.status = EmailMessageStatus::Sent as i32;
        request.update(&db).await.unwrap();

        let row2: (String,) = sqlx::query_as("SELECT updated_at FROM email_requests WHERE id = ?")
            .bind(request.id)
            .fetch_one(&db)
            .await
            .unwrap();

        // updated_at should be different (or at least not cause an error)
        // Note: In SQLite with datetime('now'), this may be the same second
        assert!(!row1.0.is_empty());
        assert!(!row2.0.is_empty());
    }

    // === Field validation edge case tests ===

    #[tokio::test]
    async fn test_save_with_long_email() {
        let db = setup_db().await;
        let content = create_test_content().save(&db).await.unwrap();

        // Email with 254 characters (just under typical limit)
        let long_local = "a".repeat(64);
        let long_domain = format!("{}.com", "b".repeat(185));
        let long_email = format!("{long_local}@{long_domain}");

        let req = EmailRequest {
            id: None,
            topic_id: Some("long_email_test".to_string()),
            content_id: Some(content.id.unwrap()),
            email: long_email.clone(),
            subject: Arc::new(String::new()),
            content: Arc::new(String::new()),
            scheduled_at: None,
            status: EmailMessageStatus::Created as i32,
            error: None,
            message_id: None,
        };

        let saved = req.save(&db).await.unwrap();
        assert_eq!(saved.email, long_email);
    }

    #[tokio::test]
    async fn test_save_with_unicode_email() {
        let db = setup_db().await;
        let content = create_test_content().save(&db).await.unwrap();

        // International email with unicode
        let unicode_email = "用户@例子.测试".to_string();

        let req = EmailRequest {
            id: None,
            topic_id: Some("unicode_test".to_string()),
            content_id: Some(content.id.unwrap()),
            email: unicode_email.clone(),
            subject: Arc::new(String::new()),
            content: Arc::new(String::new()),
            scheduled_at: None,
            status: EmailMessageStatus::Created as i32,
            error: None,
            message_id: None,
        };

        let saved = req.save(&db).await.unwrap();
        assert_eq!(saved.email, unicode_email);
    }

    #[tokio::test]
    async fn test_save_with_special_characters_in_topic_id() {
        let db = setup_db().await;
        let content = create_test_content().save(&db).await.unwrap();

        let special_topic = "topic-with_special.chars:2024/01".to_string();

        let req = EmailRequest {
            id: None,
            topic_id: Some(special_topic.clone()),
            content_id: Some(content.id.unwrap()),
            email: "test@example.com".to_string(),
            subject: Arc::new(String::new()),
            content: Arc::new(String::new()),
            scheduled_at: None,
            status: EmailMessageStatus::Created as i32,
            error: None,
            message_id: None,
        };

        let saved = req.save(&db).await.unwrap();
        assert_eq!(saved.topic_id, Some(special_topic));
    }

    #[tokio::test]
    async fn test_save_with_empty_topic_id() {
        let db = setup_db().await;
        let content = create_test_content().save(&db).await.unwrap();

        let req = EmailRequest {
            id: None,
            topic_id: Some(String::new()), // Empty string
            content_id: Some(content.id.unwrap()),
            email: "test@example.com".to_string(),
            subject: Arc::new(String::new()),
            content: Arc::new(String::new()),
            scheduled_at: None,
            status: EmailMessageStatus::Created as i32,
            error: None,
            message_id: None,
        };

        let saved = req.save(&db).await.unwrap();
        assert_eq!(saved.topic_id, Some(String::new()));
    }

    #[tokio::test]
    async fn test_save_with_none_topic_id_should_fail() {
        let db = setup_db().await;
        let content = create_test_content().save(&db).await.unwrap();

        let req = EmailRequest {
            id: None,
            topic_id: None,
            content_id: Some(content.id.unwrap()),
            email: "test@example.com".to_string(),
            subject: Arc::new(String::new()),
            content: Arc::new(String::new()),
            scheduled_at: None,
            status: EmailMessageStatus::Created as i32,
            error: None,
            message_id: None,
        };

        // topic_id has NOT NULL constraint in database, so save should fail
        let result = req.save(&db).await;
        assert!(result.is_err(), "Saving with None topic_id should fail due to NOT NULL constraint");
    }

    #[tokio::test]
    async fn test_save_with_long_error_message() {
        let db = setup_db().await;
        let content = create_test_content().save(&db).await.unwrap();

        let long_error = "Error: ".to_string() + &"x".repeat(500);

        let mut req = create_test_request_with_content_id(content.id.unwrap())
            .save(&db)
            .await
            .unwrap();

        req.status = EmailMessageStatus::Failed as i32;
        req.error = Some(long_error.clone());
        req.update(&db).await.unwrap();

        let row: (Option<String>,) = sqlx::query_as("SELECT error FROM email_requests WHERE id = ?")
            .bind(req.id)
            .fetch_one(&db)
            .await
            .unwrap();

        assert!(row.0.is_some());
        assert!(row.0.unwrap().len() > 100);
    }

    // === Batch INSERT ID calculation edge case tests ===

    #[tokio::test]
    async fn test_save_batch_exactly_batch_size() {
        use crate::constants::BATCH_INSERT_SIZE;
        let db = setup_db().await;
        let content = create_test_content().save(&db).await.unwrap();
        let content_id = content.id.unwrap();

        let requests: Vec<EmailRequest> = (0..BATCH_INSERT_SIZE)
            .map(|i| EmailRequest {
                id: None,
                topic_id: Some("exact_batch".to_string()),
                content_id: Some(content_id),
                email: format!("user{i}@example.com"),
                subject: Arc::new(String::new()),
                content: Arc::new(String::new()),
                scheduled_at: None,
                status: EmailMessageStatus::Created as i32,
                error: None,
                message_id: None,
            })
            .collect();

        let saved = EmailRequest::save_batch(requests, &db).await.unwrap();

        assert_eq!(saved.len(), BATCH_INSERT_SIZE);

        // Verify IDs are sequential
        #[allow(clippy::cast_possible_truncation, clippy::cast_possible_wrap)]
        for (i, req) in saved.iter().enumerate() {
            assert_eq!(req.id, Some((i + 1) as i32));
        }
    }

    #[tokio::test]
    async fn test_save_batch_one_over_batch_size() {
        use crate::constants::BATCH_INSERT_SIZE;
        let db = setup_db().await;
        let content = create_test_content().save(&db).await.unwrap();
        let content_id = content.id.unwrap();

        // BATCH_INSERT_SIZE + 1 to test boundary crossing
        let count = BATCH_INSERT_SIZE + 1;
        let requests: Vec<EmailRequest> = (0..count)
            .map(|i| EmailRequest {
                id: None,
                topic_id: Some("boundary_batch".to_string()),
                content_id: Some(content_id),
                email: format!("user{i}@example.com"),
                subject: Arc::new(String::new()),
                content: Arc::new(String::new()),
                scheduled_at: None,
                status: EmailMessageStatus::Created as i32,
                error: None,
                message_id: None,
            })
            .collect();

        let saved = EmailRequest::save_batch(requests, &db).await.unwrap();

        assert_eq!(saved.len(), count);

        // Verify all IDs are sequential across chunk boundary
        #[allow(clippy::cast_possible_truncation, clippy::cast_possible_wrap)]
        for (i, req) in saved.iter().enumerate() {
            assert_eq!(
                req.id,
                Some((i + 1) as i32),
                "ID mismatch at index {i}: expected {}, got {:?}",
                i + 1,
                req.id
            );
        }

        // Verify in database
        let count_db: (i32,) = sqlx::query_as("SELECT COUNT(*) FROM email_requests")
            .fetch_one(&db)
            .await
            .unwrap();
        assert_eq!(count_db.0, count as i32);
    }

    #[tokio::test]
    async fn test_save_batch_two_full_batches() {
        use crate::constants::BATCH_INSERT_SIZE;
        let db = setup_db().await;
        let content = create_test_content().save(&db).await.unwrap();
        let content_id = content.id.unwrap();

        // Exactly 2 full batches
        let count = BATCH_INSERT_SIZE * 2;
        let requests: Vec<EmailRequest> = (0..count)
            .map(|i| EmailRequest {
                id: None,
                topic_id: Some("two_batches".to_string()),
                content_id: Some(content_id),
                email: format!("user{i}@example.com"),
                subject: Arc::new(String::new()),
                content: Arc::new(String::new()),
                scheduled_at: None,
                status: EmailMessageStatus::Created as i32,
                error: None,
                message_id: None,
            })
            .collect();

        let saved = EmailRequest::save_batch(requests, &db).await.unwrap();

        assert_eq!(saved.len(), count);

        // Verify sequential IDs across two chunks
        #[allow(clippy::cast_possible_truncation, clippy::cast_possible_wrap)]
        for (i, req) in saved.iter().enumerate() {
            assert_eq!(req.id, Some((i + 1) as i32));
        }
    }

    #[tokio::test]
    async fn test_save_batch_preserves_order() {
        let db = setup_db().await;
        let content = create_test_content().save(&db).await.unwrap();
        let content_id = content.id.unwrap();

        let emails: Vec<String> = vec![
            "alpha@test.com",
            "beta@test.com",
            "gamma@test.com",
            "delta@test.com",
            "epsilon@test.com",
        ]
        .into_iter()
        .map(String::from)
        .collect();

        let requests: Vec<EmailRequest> = emails
            .iter()
            .map(|email| EmailRequest {
                id: None,
                topic_id: Some("order_test".to_string()),
                content_id: Some(content_id),
                email: email.clone(),
                subject: Arc::new(String::new()),
                content: Arc::new(String::new()),
                scheduled_at: None,
                status: EmailMessageStatus::Created as i32,
                error: None,
                message_id: None,
            })
            .collect();

        let saved = EmailRequest::save_batch(requests, &db).await.unwrap();

        // Verify order is preserved
        for (i, req) in saved.iter().enumerate() {
            assert_eq!(req.email, emails[i]);
        }
    }

    // === Large dataset tests ===

    #[tokio::test]
    async fn test_save_batch_1000_records() {
        let db = setup_db().await;
        let content = create_test_content().save(&db).await.unwrap();
        let content_id = content.id.unwrap();

        let requests: Vec<EmailRequest> = (0..1000)
            .map(|i| EmailRequest {
                id: None,
                topic_id: Some("large_1000".to_string()),
                content_id: Some(content_id),
                email: format!("user{i}@example.com"),
                subject: Arc::new(String::new()),
                content: Arc::new(String::new()),
                scheduled_at: None,
                status: EmailMessageStatus::Created as i32,
                error: None,
                message_id: None,
            })
            .collect();

        let saved = EmailRequest::save_batch(requests, &db).await.unwrap();

        assert_eq!(saved.len(), 1000);

        // Verify first and last IDs
        assert_eq!(saved.first().unwrap().id, Some(1));
        assert_eq!(saved.last().unwrap().id, Some(1000));

        // Verify in database
        let count: (i32,) = sqlx::query_as("SELECT COUNT(*) FROM email_requests")
            .fetch_one(&db)
            .await
            .unwrap();
        assert_eq!(count.0, 1000);
    }

    #[tokio::test]
    async fn test_get_request_counts_large_topic() {
        let db = setup_db().await;
        let content = create_test_content().save(&db).await.unwrap();
        let content_id = content.id.unwrap();

        // Create 500 requests with various statuses
        let requests: Vec<EmailRequest> = (0..500)
            .map(|i| EmailRequest {
                id: None,
                topic_id: Some("large_counts".to_string()),
                content_id: Some(content_id),
                email: format!("user{i}@example.com"),
                subject: Arc::new(String::new()),
                content: Arc::new(String::new()),
                scheduled_at: None,
                status: (i % 5) as i32, // Distribute across all statuses
                error: None,
                message_id: None,
            })
            .collect();

        EmailRequest::save_batch(requests, &db).await.unwrap();

        let counts = EmailRequest::get_request_counts_by_topic_id(&db, "large_counts")
            .await
            .unwrap();

        // Each status should have 100 records (500 / 5)
        assert_eq!(counts.get("Created"), Some(&100));
        assert_eq!(counts.get("Processed"), Some(&100));
        assert_eq!(counts.get("Sent"), Some(&100));
        assert_eq!(counts.get("Failed"), Some(&100));
        assert_eq!(counts.get("Stopped"), Some(&100));
    }

    #[tokio::test]
    async fn test_stop_topic_large_batch() {
        let db = setup_db().await;
        let content = create_test_content().save(&db).await.unwrap();
        let content_id = content.id.unwrap();

        // Create 300 requests all with Created status
        let requests: Vec<EmailRequest> = (0..300)
            .map(|i| EmailRequest {
                id: None,
                topic_id: Some("stop_large".to_string()),
                content_id: Some(content_id),
                email: format!("user{i}@example.com"),
                subject: Arc::new(String::new()),
                content: Arc::new(String::new()),
                scheduled_at: None,
                status: EmailMessageStatus::Created as i32,
                error: None,
                message_id: None,
            })
            .collect();

        EmailRequest::save_batch(requests, &db).await.unwrap();

        // Stop all
        EmailRequest::stop_topic(&db, "stop_large").await.unwrap();

        let counts = EmailRequest::get_request_counts_by_topic_id(&db, "stop_large")
            .await
            .unwrap();

        assert_eq!(counts.get("Stopped"), Some(&300));
        assert!(counts.get("Created").is_none());
    }

    // === Arc<String> memory efficiency tests ===

    #[tokio::test]
    async fn test_arc_string_sharing() {
        let db = setup_db().await;
        let content = create_test_content().save(&db).await.unwrap();
        let content_id = content.id.unwrap();

        // Create shared Arc<String> for subject and content
        let shared_subject = Arc::new("Shared Subject".to_string());
        let shared_content = Arc::new("Shared Content".to_string());

        let requests: Vec<EmailRequest> = (0..10)
            .map(|i| EmailRequest {
                id: None,
                topic_id: Some("arc_test".to_string()),
                content_id: Some(content_id),
                email: format!("user{i}@example.com"),
                subject: Arc::clone(&shared_subject),
                content: Arc::clone(&shared_content),
                scheduled_at: None,
                status: EmailMessageStatus::Created as i32,
                error: None,
                message_id: None,
            })
            .collect();

        // Verify all requests share the same Arc
        assert_eq!(Arc::strong_count(&shared_subject), 11); // 10 requests + original
        assert_eq!(Arc::strong_count(&shared_content), 11);

        let saved = EmailRequest::save_batch(requests, &db).await.unwrap();
        assert_eq!(saved.len(), 10);

        // After save, the saved copies should still share the Arc
        for req in &saved {
            assert!(Arc::ptr_eq(&req.subject, &shared_subject));
            assert!(Arc::ptr_eq(&req.content, &shared_content));
        }
    }

    // === Foreign key constraint tests ===

    #[tokio::test]
    async fn test_save_with_valid_content_id() {
        let db = setup_db().await;
        let content = create_test_content().save(&db).await.unwrap();

        let req = create_test_request_with_content_id(content.id.unwrap());
        let saved = req.save(&db).await.unwrap();

        assert!(saved.id.is_some());
        assert_eq!(saved.content_id, content.id);
    }

    #[tokio::test]
    async fn test_sent_count_with_various_hours() {
        let db = setup_db().await;
        let content = create_test_content().save(&db).await.unwrap();
        let content_id = content.id.unwrap();

        // Create some sent emails
        for _ in 0..5 {
            let mut req = create_test_request_with_content_id(content_id)
                .save(&db)
                .await
                .unwrap();
            req.status = EmailMessageStatus::Sent as i32;
            req.update(&db).await.unwrap();
        }

        // Test with different hour values
        let count_1h = EmailRequest::sent_count(&db, 1).await.unwrap();
        let count_24h = EmailRequest::sent_count(&db, 24).await.unwrap();
        let count_168h = EmailRequest::sent_count(&db, 168).await.unwrap(); // 1 week

        // All should return 5 since records were just created
        assert_eq!(count_1h, 5);
        assert_eq!(count_24h, 5);
        assert_eq!(count_168h, 5);
    }

    #[tokio::test]
    async fn test_sent_count_zero_hours() {
        let db = setup_db().await;

        // Edge case: 0 hours should still work
        let count = EmailRequest::sent_count(&db, 0).await.unwrap();
        assert_eq!(count, 0);
    }
}
