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
}
