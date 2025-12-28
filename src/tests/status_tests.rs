#[cfg(test)]
mod tests {
    use crate::models::request::EmailMessageStatus;

    #[test]
    fn test_from_i32_valid() {
        assert_eq!(
            EmailMessageStatus::from_i32(0),
            Some(EmailMessageStatus::Created)
        );
        assert_eq!(
            EmailMessageStatus::from_i32(1),
            Some(EmailMessageStatus::Processed)
        );
        assert_eq!(
            EmailMessageStatus::from_i32(2),
            Some(EmailMessageStatus::Sent)
        );
        assert_eq!(
            EmailMessageStatus::from_i32(3),
            Some(EmailMessageStatus::Failed)
        );
        assert_eq!(
            EmailMessageStatus::from_i32(4),
            Some(EmailMessageStatus::Stopped)
        );
    }

    #[test]
    fn test_from_i32_invalid() {
        assert_eq!(EmailMessageStatus::from_i32(-1), None);
        assert_eq!(EmailMessageStatus::from_i32(5), None);
        assert_eq!(EmailMessageStatus::from_i32(100), None);
    }

    #[test]
    fn test_as_str() {
        assert_eq!(EmailMessageStatus::Created.as_str(), "Created");
        assert_eq!(EmailMessageStatus::Processed.as_str(), "Processed");
        assert_eq!(EmailMessageStatus::Sent.as_str(), "Sent");
        assert_eq!(EmailMessageStatus::Failed.as_str(), "Failed");
        assert_eq!(EmailMessageStatus::Stopped.as_str(), "Stopped");
    }

    #[test]
    fn test_status_as_i32() {
        assert_eq!(EmailMessageStatus::Created as i32, 0);
        assert_eq!(EmailMessageStatus::Processed as i32, 1);
        assert_eq!(EmailMessageStatus::Sent as i32, 2);
        assert_eq!(EmailMessageStatus::Failed as i32, 3);
        assert_eq!(EmailMessageStatus::Stopped as i32, 4);
    }

    #[test]
    fn test_roundtrip() {
        for i in 0..=4 {
            let status = EmailMessageStatus::from_i32(i).unwrap();
            assert_eq!(status as i32, i);
        }
    }

    #[test]
    fn test_equality() {
        assert_eq!(EmailMessageStatus::Created, EmailMessageStatus::Created);
        assert_ne!(EmailMessageStatus::Created, EmailMessageStatus::Sent);
    }

    #[test]
    fn test_debug() {
        let debug_str = format!("{:?}", EmailMessageStatus::Sent);
        assert_eq!(debug_str, "Sent");
    }

    // === Additional edge case tests ===

    #[test]
    fn test_from_i32_boundary_values() {
        // Test boundary values around valid range
        assert_eq!(EmailMessageStatus::from_i32(-2), None);
        assert_eq!(EmailMessageStatus::from_i32(-1), None);
        assert!(EmailMessageStatus::from_i32(0).is_some());
        assert!(EmailMessageStatus::from_i32(4).is_some());
        assert_eq!(EmailMessageStatus::from_i32(5), None);
        assert_eq!(EmailMessageStatus::from_i32(6), None);
    }

    #[test]
    fn test_from_i32_extreme_values() {
        assert_eq!(EmailMessageStatus::from_i32(i32::MIN), None);
        assert_eq!(EmailMessageStatus::from_i32(i32::MAX), None);
        assert_eq!(EmailMessageStatus::from_i32(1000), None);
        assert_eq!(EmailMessageStatus::from_i32(-1000), None);
    }

    #[test]
    fn test_clone() {
        let status = EmailMessageStatus::Sent;
        let cloned = status;
        assert_eq!(status, cloned);
    }

    #[test]
    fn test_copy_semantics() {
        let status1 = EmailMessageStatus::Created;
        let status2 = status1; // Copy, not move
        assert_eq!(status1, status2);
        // Both are still usable
        assert_eq!(status1.as_str(), "Created");
        assert_eq!(status2.as_str(), "Created");
    }

    #[test]
    fn test_all_statuses_have_unique_values() {
        let statuses = [
            EmailMessageStatus::Created as i32,
            EmailMessageStatus::Processed as i32,
            EmailMessageStatus::Sent as i32,
            EmailMessageStatus::Failed as i32,
            EmailMessageStatus::Stopped as i32,
        ];

        // Check all values are unique
        for i in 0..statuses.len() {
            for j in (i + 1)..statuses.len() {
                assert_ne!(
                    statuses[i], statuses[j],
                    "Status values at index {} and {} should be different",
                    i, j
                );
            }
        }
    }

    #[test]
    fn test_all_statuses_have_unique_strings() {
        let strings = [
            EmailMessageStatus::Created.as_str(),
            EmailMessageStatus::Processed.as_str(),
            EmailMessageStatus::Sent.as_str(),
            EmailMessageStatus::Failed.as_str(),
            EmailMessageStatus::Stopped.as_str(),
        ];

        for i in 0..strings.len() {
            for j in (i + 1)..strings.len() {
                assert_ne!(
                    strings[i], strings[j],
                    "Status strings at index {} and {} should be different",
                    i, j
                );
            }
        }
    }

    #[test]
    fn test_status_values_are_contiguous() {
        // Verify status values are 0, 1, 2, 3, 4 (contiguous)
        let expected = [0, 1, 2, 3, 4];
        let actual = [
            EmailMessageStatus::Created as i32,
            EmailMessageStatus::Processed as i32,
            EmailMessageStatus::Sent as i32,
            EmailMessageStatus::Failed as i32,
            EmailMessageStatus::Stopped as i32,
        ];
        assert_eq!(expected, actual);
    }

    #[test]
    fn test_inequality_all_pairs() {
        let all_statuses = [
            EmailMessageStatus::Created,
            EmailMessageStatus::Processed,
            EmailMessageStatus::Sent,
            EmailMessageStatus::Failed,
            EmailMessageStatus::Stopped,
        ];

        for i in 0..all_statuses.len() {
            for j in 0..all_statuses.len() {
                if i == j {
                    assert_eq!(all_statuses[i], all_statuses[j]);
                } else {
                    assert_ne!(all_statuses[i], all_statuses[j]);
                }
            }
        }
    }

    #[test]
    fn test_as_str_returns_static_str() {
        // Verify as_str returns &'static str (compile-time check)
        let s: &'static str = EmailMessageStatus::Sent.as_str();
        assert!(!s.is_empty());
    }

    #[test]
    fn test_from_i32_is_const() {
        // Verify from_i32 can be used in const context
        const STATUS: Option<EmailMessageStatus> = EmailMessageStatus::from_i32(2);
        assert!(STATUS.is_some());
        assert_eq!(STATUS.unwrap() as i32, 2);
    }
}
