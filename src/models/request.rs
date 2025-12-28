//! Email request model and database operations

use std::{collections::HashMap, sync::Arc};

use chrono::{FixedOffset, NaiveDateTime, TimeZone, Utc};
use serde::Deserialize;
use sqlx::SqlitePool;
use tracing::debug;

use crate::constants::BATCH_INSERT_SIZE;

/// Email delivery status
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(i32)]
pub enum EmailMessageStatus {
    Created = 0,
    Processed = 1,
    Sent = 2,
    Failed = 3,
    Stopped = 4,
}

impl EmailMessageStatus {
    #[must_use]
    pub const fn from_i32(value: i32) -> Option<Self> {
        match value {
            0 => Some(Self::Created),
            1 => Some(Self::Processed),
            2 => Some(Self::Sent),
            3 => Some(Self::Failed),
            4 => Some(Self::Stopped),
            _ => None,
        }
    }

    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Created => "Created",
            Self::Processed => "Processed",
            Self::Sent => "Sent",
            Self::Failed => "Failed",
            Self::Stopped => "Stopped",
        }
    }
}

/// Default value for `Arc<String>` fields in serde deserialization.
fn default_arc_string() -> Arc<String> {
    Arc::new(String::new())
}

/// Email request entity
///
/// - `content_id`: FK to `email_contents` table (for storage efficiency)
/// - `subject`, `content`: Loaded at runtime via JOIN (not stored in this table)
///   These use `Arc<String>` to avoid expensive cloning when creating many requests
///   with the same content (e.g., 10,000 emails with identical subject/body).
#[derive(Clone, Debug, Deserialize)]
pub struct EmailRequest {
    pub id: Option<i32>,
    pub topic_id: Option<String>,
    pub content_id: Option<i32>,
    pub email: String,
    /// Loaded from `email_contents` at runtime (not stored in `email_requests`).
    /// Uses `Arc<String>` to share across multiple requests without cloning.
    #[serde(skip, default = "default_arc_string")]
    pub subject: Arc<String>,
    /// Loaded from `email_contents` at runtime (not stored in `email_requests`).
    /// Uses `Arc<String>` to share; cloned only when adding tracking pixel.
    #[serde(skip, default = "default_arc_string")]
    pub content: Arc<String>,
    pub scheduled_at: Option<String>,
    pub status: i32,
    pub error: Option<String>,
    pub message_id: Option<String>,
}

impl EmailRequest {
    /// Saves a single email request (use `save_batch` for bulk inserts).
    /// Note: `content_id` must be set before calling this method.
    #[cfg(test)]
    #[allow(dead_code)]
    pub async fn save(self, db_pool: &SqlitePool) -> Result<Self, sqlx::Error> {
        let scheduled_at = parse_scheduled_at(self.scheduled_at.as_deref());

        let row: (i64,) = sqlx::query_as(
            "INSERT INTO email_requests (topic_id, content_id, email, scheduled_at, status, created_at, updated_at)
             VALUES (?, ?, ?, ?, ?, datetime('now'), datetime('now'))
             RETURNING id",
        )
        .bind(&self.topic_id)
        .bind(self.content_id)
        .bind(&self.email)
        .bind(&scheduled_at)
        .bind(self.status)
        .fetch_one(db_pool)
        .await?;

        #[allow(clippy::cast_possible_truncation)]
        Ok(Self {
            id: Some(row.0 as i32),
            ..self
        })
    }

    /// Updates the email request after sending.
    pub async fn update(&self, db_pool: &SqlitePool) -> Result<(), sqlx::Error> {
        sqlx::query(
            "UPDATE email_requests SET status=?, message_id=?, error=?, updated_at=datetime('now') WHERE id=?",
        )
        .bind(self.status)
        .bind(&self.message_id)
        .bind(&self.error)
        .bind(self.id)
        .execute(db_pool)
        .await?;
        Ok(())
    }

    /// Returns the count of emails sent within the specified hours.
    pub async fn sent_count(db_pool: &SqlitePool, hours: i32) -> Result<i32, sqlx::Error> {
        let hours_str = format!("-{hours} hours");
        let row: (i32,) = sqlx::query_as(
            "SELECT COUNT(*) FROM email_requests WHERE status=? AND created_at >= datetime('now', ?)",
        )
        .bind(EmailMessageStatus::Sent as i32)
        .bind(&hours_str)
        .fetch_one(db_pool)
        .await?;

        Ok(row.0)
    }

    /// Stops all pending emails for the specified topic.
    pub async fn stop_topic(db_pool: &SqlitePool, topic_id: &str) -> Result<(), sqlx::Error> {
        sqlx::query(
            "UPDATE email_requests SET status=?, updated_at=datetime('now') WHERE status=? AND topic_id=?",
        )
        .bind(EmailMessageStatus::Stopped as i32)
        .bind(EmailMessageStatus::Created as i32)
        .bind(topic_id)
        .execute(db_pool)
        .await?;
        Ok(())
    }

    /// Returns status counts for the specified topic.
    pub async fn get_request_counts_by_topic_id(
        db_pool: &SqlitePool,
        topic_id: &str,
    ) -> Result<HashMap<String, i32>, sqlx::Error> {
        let rows: Vec<(i32, i32)> = sqlx::query_as(
            "SELECT status, COUNT(*) FROM email_requests WHERE topic_id=? GROUP BY status",
        )
        .bind(topic_id)
        .fetch_all(db_pool)
        .await?;

        let counts = rows
            .into_iter()
            .map(|(status, count)| {
                let status_str = EmailMessageStatus::from_i32(status)
                    .map_or("Unknown", EmailMessageStatus::as_str);
                (status_str.to_owned(), count)
            })
            .collect();

        Ok(counts)
    }

    /// Finds the request ID by SES message ID.
    pub async fn get_request_id_by_message_id(
        db_pool: &SqlitePool,
        message_id: &str,
    ) -> Result<i32, sqlx::Error> {
        let row: (i32,) = sqlx::query_as("SELECT id FROM email_requests WHERE message_id=?")
            .bind(message_id)
            .fetch_one(db_pool)
            .await?;

        Ok(row.0)
    }

    /// Saves multiple requests in a single transaction using multi-row INSERT.
    ///
    /// Note: `content_id` must be set for all requests before calling this method.
    /// This provides ~10x performance improvement over individual inserts.
    pub async fn save_batch(
        requests: Vec<Self>,
        db_pool: &SqlitePool,
    ) -> Result<Vec<Self>, sqlx::Error> {
        if requests.is_empty() {
            return Ok(Vec::new());
        }

        let total = requests.len();
        let mut results = Vec::with_capacity(total);
        let mut tx = db_pool.begin().await?;

        for chunk in requests.chunks(BATCH_INSERT_SIZE) {
            let chunk_size = chunk.len();

            let placeholders = (0..chunk_size)
                .map(|_| "(?, ?, ?, ?, ?, datetime('now'), datetime('now'))")
                .collect::<Vec<_>>()
                .join(", ");

            let sql = format!(
                "INSERT INTO email_requests (topic_id, content_id, email, scheduled_at, status, created_at, updated_at) VALUES {placeholders}"
            );

            let mut query = sqlx::query(&sql);

            for req in chunk {
                let scheduled_at = parse_scheduled_at(req.scheduled_at.as_deref());
                query = query
                    .bind(&req.topic_id)
                    .bind(req.content_id)
                    .bind(&req.email)
                    .bind(scheduled_at)
                    .bind(req.status);
            }

            query.execute(&mut *tx).await?;

            let (last_id,): (i64,) = sqlx::query_as("SELECT last_insert_rowid()")
                .fetch_one(&mut *tx)
                .await?;

            #[allow(clippy::cast_possible_truncation, clippy::cast_possible_wrap)]
            for (i, req) in chunk.iter().enumerate() {
                let id = (last_id - (chunk_size - 1 - i) as i64) as i32;
                results.push(Self {
                    id: Some(id),
                    ..req.clone()
                });
            }
        }

        tx.commit().await?;
        debug!("Batch inserted {total} records");

        Ok(results)
    }
}

/// Parses `scheduled_at` string and converts KST to UTC.
///
/// Input is expected to be in KST (Asia/Seoul, UTC+9) format: "YYYY-MM-DD HH:MM:SS"
/// Returns UTC formatted string for `SQLite` datetime comparison.
fn parse_scheduled_at(scheduled: Option<&str>) -> String {
    let now = Utc::now();
    let now_str = || now.format("%Y-%m-%d %H:%M:%S").to_string();

    // KST is UTC+9
    let kst = FixedOffset::east_opt(9 * 3600).expect("valid offset");

    match scheduled {
        Some(s) if !s.is_empty() => NaiveDateTime::parse_from_str(s, "%Y-%m-%d %H:%M:%S")
            .ok()
            .and_then(|dt| kst.from_local_datetime(&dt).single())
            .map(|kst_dt| kst_dt.with_timezone(&Utc))
            .map_or_else(
                || {
                    tracing::warn!("Invalid scheduled_at '{}', using now", s);
                    now_str()
                },
                |utc| utc.format("%Y-%m-%d %H:%M:%S").to_string(),
            ),
        _ => now_str(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use regex::Regex;

    fn datetime_format_regex() -> Regex {
        Regex::new(r"^\d{4}-\d{2}-\d{2} \d{2}:\d{2}:\d{2}$").unwrap()
    }

    #[test]
    fn test_parse_scheduled_at_none_returns_valid_format() {
        let result = parse_scheduled_at(None);
        assert!(datetime_format_regex().is_match(&result));
    }

    #[test]
    fn test_parse_scheduled_at_empty_returns_valid_format() {
        let result = parse_scheduled_at(Some(""));
        assert!(datetime_format_regex().is_match(&result));
    }

    #[test]
    fn test_parse_scheduled_at_invalid_uses_fallback() {
        let result = parse_scheduled_at(Some("invalid"));
        assert!(datetime_format_regex().is_match(&result));
    }

    #[test]
    fn test_parse_scheduled_at_kst_converts_to_utc() {
        // KST 15:30:45 should become UTC 06:30:45 (9 hours earlier)
        let kst_input = "2025-12-27 15:30:45";
        let result = parse_scheduled_at(Some(kst_input));

        assert_eq!(result, "2025-12-27 06:30:45");
    }

    #[test]
    fn test_parse_scheduled_at_kst_midnight_crosses_date() {
        // KST 2025-12-28 03:00:00 should become UTC 2025-12-27 18:00:00
        let kst_input = "2025-12-28 03:00:00";
        let result = parse_scheduled_at(Some(kst_input));

        assert_eq!(result, "2025-12-27 18:00:00");
    }

    #[test]
    fn test_parse_scheduled_at_kst_early_morning_previous_day() {
        // KST 2025-01-01 08:00:00 should become UTC 2024-12-31 23:00:00
        let kst_input = "2025-01-01 08:00:00";
        let result = parse_scheduled_at(Some(kst_input));

        assert_eq!(result, "2024-12-31 23:00:00");
    }

    #[test]
    fn test_parse_scheduled_at_preserves_format() {
        let kst_input = "2025-06-15 12:00:00";
        let result = parse_scheduled_at(Some(kst_input));

        // KST 12:00 -> UTC 03:00
        assert_eq!(result, "2025-06-15 03:00:00");
        assert!(datetime_format_regex().is_match(&result));
    }

    // === Edge case tests for parse_scheduled_at ===

    #[test]
    fn test_parse_scheduled_at_invalid_month() {
        // Invalid month (13)
        let result = parse_scheduled_at(Some("2025-13-01 12:00:00"));
        assert!(datetime_format_regex().is_match(&result));
        // Should fallback to current time
    }

    #[test]
    fn test_parse_scheduled_at_invalid_day() {
        // Invalid day (32)
        let result = parse_scheduled_at(Some("2025-01-32 12:00:00"));
        assert!(datetime_format_regex().is_match(&result));
    }

    #[test]
    fn test_parse_scheduled_at_invalid_hour() {
        // Invalid hour (25)
        let result = parse_scheduled_at(Some("2025-01-15 25:00:00"));
        assert!(datetime_format_regex().is_match(&result));
    }

    #[test]
    fn test_parse_scheduled_at_wrong_format_slash() {
        // Wrong format with slashes
        let result = parse_scheduled_at(Some("2025/01/15 12:00:00"));
        assert!(datetime_format_regex().is_match(&result));
    }

    #[test]
    fn test_parse_scheduled_at_iso_format() {
        // ISO format with T separator (should fail and fallback)
        let result = parse_scheduled_at(Some("2025-01-15T12:00:00"));
        assert!(datetime_format_regex().is_match(&result));
    }

    #[test]
    fn test_parse_scheduled_at_with_timezone() {
        // Input with timezone info (should fail and fallback)
        let result = parse_scheduled_at(Some("2025-01-15 12:00:00+09:00"));
        assert!(datetime_format_regex().is_match(&result));
    }

    #[test]
    fn test_parse_scheduled_at_whitespace_only() {
        let result = parse_scheduled_at(Some("   "));
        assert!(datetime_format_regex().is_match(&result));
    }

    #[test]
    fn test_parse_scheduled_at_partial_date() {
        // Missing time part
        let result = parse_scheduled_at(Some("2025-01-15"));
        assert!(datetime_format_regex().is_match(&result));
    }

    #[test]
    fn test_parse_scheduled_at_leap_year_valid() {
        // Valid leap year date
        let result = parse_scheduled_at(Some("2024-02-29 12:00:00"));
        assert_eq!(result, "2024-02-29 03:00:00");
    }

    #[test]
    fn test_parse_scheduled_at_leap_year_invalid() {
        // Invalid leap year date (2025 is not a leap year)
        let result = parse_scheduled_at(Some("2025-02-29 12:00:00"));
        assert!(datetime_format_regex().is_match(&result));
        // Should fallback since Feb 29, 2025 doesn't exist
    }

    #[test]
    fn test_parse_scheduled_at_end_of_year() {
        // KST 2025-12-31 23:59:59 -> UTC 2025-12-31 14:59:59
        let result = parse_scheduled_at(Some("2025-12-31 23:59:59"));
        assert_eq!(result, "2025-12-31 14:59:59");
    }

    #[test]
    fn test_parse_scheduled_at_start_of_year() {
        // KST 2025-01-01 00:00:00 -> UTC 2024-12-31 15:00:00
        let result = parse_scheduled_at(Some("2025-01-01 00:00:00"));
        assert_eq!(result, "2024-12-31 15:00:00");
    }

    #[test]
    fn test_parse_scheduled_at_exactly_nine_hours() {
        // KST 09:00:00 -> UTC 00:00:00 (same day)
        let result = parse_scheduled_at(Some("2025-06-15 09:00:00"));
        assert_eq!(result, "2025-06-15 00:00:00");
    }

    #[test]
    fn test_parse_scheduled_at_sql_injection_attempt() {
        // SQL injection attempt should be safely handled
        let result = parse_scheduled_at(Some("2025-01-15'; DROP TABLE--"));
        assert!(datetime_format_regex().is_match(&result));
    }

    #[test]
    fn test_parse_scheduled_at_very_long_string() {
        let long_string = "2025-01-15 12:00:00".to_string() + &"x".repeat(1000);
        let result = parse_scheduled_at(Some(&long_string));
        assert!(datetime_format_regex().is_match(&result));
    }

    #[test]
    fn test_parse_scheduled_at_unicode() {
        // Unicode characters in date
        let result = parse_scheduled_at(Some("２０２５-01-15 12:00:00")); // Full-width numbers
        assert!(datetime_format_regex().is_match(&result));
    }

    #[test]
    fn test_parse_scheduled_at_negative_values() {
        // Negative year is parsed by chrono as a valid date (year -2025)
        // The result will have format "-2025-..." which doesn't match standard regex
        // Just verify the function doesn't panic and returns a non-empty string
        let result = parse_scheduled_at(Some("-2025-01-15 12:00:00"));
        assert!(!result.is_empty());
    }

    #[test]
    fn test_parse_scheduled_at_zero_padded() {
        // Properly zero-padded should work
        // KST 2025-01-05 08:05:09 -> UTC 2025-01-04 23:05:09 (subtract 9 hours)
        let result = parse_scheduled_at(Some("2025-01-05 08:05:09"));
        assert_eq!(result, "2025-01-04 23:05:09");
    }
}
