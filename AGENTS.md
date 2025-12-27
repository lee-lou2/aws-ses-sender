# AGENTS.md

> Project Guide for AI Coding Agents

---

## 📋 Project Overview

**aws-ses-sender** is a high-performance bulk email sending service powered by AWS SES.

### Core Features

- 🚀 **Bulk Email Sending**: Up to 10,000 emails per request
- ⏰ **Scheduled Delivery**: Future sending via `scheduled_at` field
- 📊 **Event Tracking**: Bounce/Complaint/Delivery events via AWS SNS
- 👀 **Open Tracking**: 1x1 transparent pixel for email open detection
- ⚡ **Rate Limiting**: Token Bucket + Semaphore for per-second rate control

### Tech Stack

| Area | Technology |
|------|------------|
| Language | Rust 2021 Edition |
| Web Framework | Axum 0.8 |
| Async Runtime | Tokio |
| Database | SQLite (WAL mode) |
| Email Service | AWS SES v2 |
| Authentication | X-API-KEY header |
| Error Tracking | Sentry |

---

## 🏗 Architecture

```
┌─────────────┐     ┌─────────────┐     ┌─────────────┐     ┌─────────────┐
│  API Server │────▶│  Scheduler  │────▶│   Sender    │────▶│  AWS SES    │
│   (Axum)    │     │  (Batch)    │     │ (Rate Limit)│     │             │
└─────────────┘     └─────────────┘     └─────────────┘     └─────────────┘
       │                   │                   │                   │
       │                   ▼                   ▼                   ▼
       │            ┌─────────────┐     ┌─────────────┐     ┌─────────────┐
       └───────────▶│   SQLite    │◀────│ Post-Proc   │◀────│   AWS SNS   │
                    │   (WAL)     │     │  (Batch)    │     │  (Events)   │
                    └─────────────┘     └─────────────┘     └─────────────┘
```

### Data Flow

1. **Immediate Sending**: API → Batch INSERT → Send Channel → Rate-limited Sending → Batch Result Update
2. **Scheduled Sending**: API → Batch INSERT (Created) → Scheduler Polling → Send Channel → Sending → Result Update

### Background Tasks (3 concurrent tasks)

| Task | Function | Purpose |
|------|----------|---------|
| Scheduler | `schedule_pre_send_message` | Polls for scheduled emails every 10s |
| Sender | `receive_send_message` | Rate-limited email sending via Token Bucket |
| Post-Processor | `receive_post_send_message` | Batches and persists sending results |

---

## 📁 Project Structure

```
src/
├── main.rs                 # Entry point, initialization, background task spawning
├── app.rs                  # Axum router configuration
├── config.rs               # Environment variable loading (singleton)
├── state.rs                # AppState definition (DB pool, channels)
├── handlers/               # HTTP request handlers
│   ├── mod.rs
│   ├── message_handlers.rs # POST /v1/messages
│   ├── event_handlers.rs   # GET/POST /v1/events/*
│   └── topic_handlers.rs   # GET/DELETE /v1/topics/{id}
├── services/               # Background services
│   ├── mod.rs
│   ├── scheduler.rs        # Scheduled email polling (10s interval)
│   ├── receiver.rs         # Rate-limited sending + batch DB updates
│   └── sender.rs           # AWS SES API calls (singleton client)
├── models/                 # Data models
│   ├── mod.rs
│   ├── content.rs          # Email content model
│   ├── request.rs          # EmailRequest, EmailMessageStatus
│   └── result.rs           # EmailResult
├── middlewares/            # HTTP middlewares
│   ├── mod.rs
│   └── auth_middlewares.rs # API Key authentication
└── tests/                  # Test modules
    ├── mod.rs              # Shared helper functions
    ├── auth_tests.rs       # Authentication tests
    ├── event_tests.rs      # Event handler tests
    ├── handler_tests.rs    # Message/topic handler tests
    ├── request_tests.rs    # EmailRequest model tests
    └── status_tests.rs     # EmailMessageStatus enum tests
```

---

## 🔑 Core Modules

### `src/main.rs`

Entry point and initialization:
- Logger and Sentry setup
- Database pool creation with SQLite optimizations
- Channel creation for inter-task communication
- Spawning 3 background tasks

Key constants:
```rust
const DB_MAX_CONNECTIONS: u32 = 20;
const DB_MIN_CONNECTIONS: u32 = 5;
const SEND_CHANNEL_BUFFER: usize = 10_000;
const POST_SEND_CHANNEL_BUFFER: usize = 1_000;
```

### `src/services/receiver.rs`

**Most complex module** - handles rate limiting and concurrency control.

```rust
// Token Bucket: per-second rate control
let tokens = Arc::new(AtomicU64::new(max_per_sec));

// Semaphore: concurrent request limit (max_per_sec * 2)
let semaphore = Arc::new(Semaphore::new(max_per_sec * 2));
```

Key constants:
```rust
const TOKEN_REFILL_INTERVAL_MS: u64 = 100;  // 10% refill every 100ms
const TOKEN_WAIT_INTERVAL_MS: u64 = 5;       // Wait between token checks
const BATCH_SIZE: usize = 100;               // Results per batch update
const BATCH_FLUSH_INTERVAL_MS: u64 = 500;    // Max wait before flush
```

### `src/services/scheduler.rs`

Polls for scheduled emails and forwards to sending queue:
```rust
const BATCH_SIZE: i32 = 1000;        // Records per poll
const IDLE_DELAY_SECS: u64 = 10;     // Delay when no messages
const ERROR_BACKOFF_SECS: u64 = 5;   // Delay after error
```

### `src/models/request.rs`

```rust
pub enum EmailMessageStatus {
    Created = 0,    // Created (waiting for scheduled time)
    Processed = 1,  // Processed (added to send queue)
    Sent = 2,       // Successfully sent
    Failed = 3,     // Send failed
    Stopped = 4,    // Cancelled by user
}
```

Key constant:
```rust
const BATCH_INSERT_SIZE: usize = 100;  // Max rows per multi-row INSERT
```

---

## 🗄 Database Schema

### `email_requests` Table

| Column | Type | Description |
|--------|------|-------------|
| id | INTEGER PK | Auto-increment ID |
| topic_id | VARCHAR(255) | Group sending identifier |
| message_id | VARCHAR(255) | AWS SES message ID |
| email | VARCHAR(255) | Recipient email address |
| subject | VARCHAR(255) | Email subject |
| content | TEXT | HTML body |
| scheduled_at | DATETIME | Scheduled send time (UTC) |
| status | TINYINT | EmailMessageStatus value |
| error | VARCHAR(255) | Error message (if failed) |
| created_at | DATETIME | Creation timestamp |
| updated_at | DATETIME | Last update timestamp |

### `email_results` Table

| Column | Type | Description |
|--------|------|-------------|
| id | INTEGER PK | Auto-increment ID |
| request_id | INTEGER FK | References email_requests.id |
| status | VARCHAR(50) | Event type (Open, Bounce, Complaint, Delivery) |
| raw | TEXT | Original SNS JSON payload |
| created_at | DATETIME | Creation timestamp |

### Indexes

```sql
CREATE INDEX idx_requests_status ON email_requests(status);
CREATE INDEX idx_requests_topic_id ON email_requests(topic_id);
CREATE INDEX idx_requests_scheduled ON email_requests(status, scheduled_at);
```

---

## 🌐 API Endpoints

| Method | Path | Auth | Handler | Description |
|--------|------|------|---------|-------------|
| POST | `/v1/messages` | ✅ | `create_message` | Send emails (immediate or scheduled) |
| GET | `/v1/topics/{topic_id}` | ✅ | `get_topic` | Get topic statistics |
| DELETE | `/v1/topics/{topic_id}` | ✅ | `stop_topic` | Cancel pending emails |
| GET | `/v1/events/open` | ❌ | `track_open` | Track email opens (returns 1x1 PNG) |
| GET | `/v1/events/counts/sent` | ✅ | `get_sent_count` | Get sent count (last N hours) |
| POST | `/v1/events/results` | ❌ | `handle_sns_event` | Receive AWS SNS events |

### Request/Response Examples

**POST /v1/messages**
```json
// Request
{
  "messages": [
    {
      "topic_id": "newsletter_2024_01",
      "emails": ["user1@example.com", "user2@example.com"],
      "subject": "January Newsletter",
      "content": "<h1>Hello!</h1><p>...</p>"
    }
  ],
  "scheduled_at": "2024-01-01 09:00:00"
}

// Response
{
  "total": 2,
  "success": 2,
  "errors": 0,
  "duration_ms": 45,
  "scheduled": true
}
```

**GET /v1/topics/{topic_id}**
```json
// Response
{
  "request_counts": {
    "Created": 100,
    "Sent": 850,
    "Failed": 5
  },
  "result_counts": {
    "Open": 423,
    "Bounce": 3,
    "Delivery": 847
  }
}
```

---

## ⚙️ Environment Variables

| Variable | Required | Default | Description |
|----------|----------|---------|-------------|
| `SERVER_PORT` | ❌ | 8080 | Server port |
| `SERVER_URL` | ✅ | - | External access URL (for tracking pixel) |
| `API_KEY` | ✅ | - | API authentication key |
| `AWS_REGION` | ❌ | ap-northeast-2 | AWS region |
| `AWS_SES_FROM_EMAIL` | ✅ | - | Sender email address |
| `MAX_SEND_PER_SECOND` | ❌ | 24 | Max emails per second |
| `SENTRY_DSN` | ❌ | - | Sentry DSN for error tracking |
| `RUST_LOG` | ❌ | info | Log level (debug, info, warn, error) |

---

## 🔧 Development Environment

### Build & Run

```bash
# Development mode
cargo run

# Release mode
cargo run --release

# Run tests
cargo test

# Run tests with output
cargo test -- --nocapture

# Linting
cargo clippy

# Formatting
cargo fmt
```

### Key Constants Summary

| Constant | Value | Location | Purpose |
|----------|-------|----------|---------|
| `DB_MAX_CONNECTIONS` | 20 | main.rs | Max database connections |
| `DB_MIN_CONNECTIONS` | 5 | main.rs | Min database connections |
| `SEND_CHANNEL_BUFFER` | 10,000 | main.rs | Send queue buffer size |
| `POST_SEND_CHANNEL_BUFFER` | 1,000 | main.rs | Post-send queue buffer |
| `BATCH_SIZE` (scheduler) | 1,000 | scheduler.rs | Emails per scheduler poll |
| `BATCH_INSERT_SIZE` | 100 | request.rs | Rows per multi-row INSERT |
| `BATCH_SIZE` (receiver) | 100 | receiver.rs | Results per batch update |
| `BATCH_FLUSH_INTERVAL_MS` | 500 | receiver.rs | Max flush wait time |
| `MAX_EMAILS_PER_REQUEST` | 10,000 | message_handlers.rs | API request limit |

---

## 📝 Rust Code Style Guide

> This project follows the [Rust Style Guide](https://doc.rust-lang.org/stable/style-guide/) and [Rust API Guidelines](https://rust-lang.github.io/api-guidelines/).

### Lint Configuration

```toml
[lints.rust]
unsafe_code = "forbid"

[lints.clippy]
all = "warn"
pedantic = "warn"
nursery = "warn"
```

### Naming Conventions

| Item | Style | Examples |
|------|-------|----------|
| Crates/Modules | `snake_case` | `email_sender`, `auth_middlewares` |
| Types/Traits | `PascalCase` | `EmailRequest`, `SendEmailError` |
| Functions/Methods | `snake_case` | `send_email`, `get_topic` |
| Constants | `SCREAMING_SNAKE_CASE` | `MAX_EMAILS_PER_REQUEST`, `BATCH_SIZE` |
| Variables/Parameters | `snake_case` | `db_pool`, `topic_id` |
| Lifetimes | Short lowercase | `'a`, `'de` |
| Type Parameters | Single uppercase or `PascalCase` | `T`, `E`, `Item` |

### Module Documentation

```rust
// ✅ Good: Single line, concise
//! Email request model and database operations

// ❌ Bad: Unnecessarily verbose
//! This module handles email request models and database operations.
//! 
//! ## Features
//! - Save email requests
//! - Query email requests
//! ...
```

### Function Documentation

```rust
// ✅ Good: Concise, only when needed
/// Saves multiple requests in a single transaction using multi-row INSERT.
///
/// Provides ~10x performance improvement over individual inserts.
pub async fn save_batch(requests: Vec<Self>, db_pool: &SqlitePool) -> Result<Vec<Self>, sqlx::Error>

// ✅ Good: Simple functions can omit documentation
pub async fn update(&self, db_pool: &SqlitePool) -> Result<(), sqlx::Error>

// ❌ Bad: Repeating what's obvious from the code
/// This function updates the email request in the database.
/// It takes a database pool and updates the request.
pub async fn update(&self, db_pool: &SqlitePool) -> Result<(), sqlx::Error>
```

### Comments

```rust
// ✅ Good: Group related constants with minimal comments
// Token bucket configuration
const TOKEN_REFILL_INTERVAL_MS: u64 = 100;
const TOKEN_WAIT_INTERVAL_MS: u64 = 5;

// Batch update configuration
const BATCH_SIZE: usize = 100;
const BATCH_FLUSH_INTERVAL_MS: u64 = 500;

// ❌ Bad: Separator comments
// =============================================================================
// Configuration
// =============================================================================
const BATCH_SIZE: usize = 100;
```

### Import Organization

```rust
// ✅ Good: std → external crates → internal modules
use std::collections::HashMap;
use std::sync::Arc;

use axum::{extract::State, http::StatusCode};
use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;

use crate::models::request::EmailRequest;
use crate::state::AppState;
```

### Error Handling

```rust
// ✅ Good: Use thiserror for custom errors
#[derive(Debug, Error)]
pub enum SendEmailError {
    #[error("Failed to build email: {0}")]
    Build(String),

    #[error("SES SDK error: {0}")]
    Sdk(String),
}

// ✅ Good: Use let-else for early returns
let Some(ses_msg_id) = ses_msg_id else {
    error!("SES message_id not found");
    return (StatusCode::BAD_REQUEST, "Not found").into_response();
};

// ✅ Good: Use ? operator for propagation
let row: (i64,) = sqlx::query_as("SELECT id FROM ...")
    .bind(message_id)
    .fetch_one(db_pool)
    .await?;
```

### Type Conversions

```rust
// ✅ Good: Explicit casting with allow attribute
#[allow(clippy::cast_possible_truncation)]
let id = row.0 as i32;

// ✅ Good: Safe conversion with fallback
let max_per_sec = u64::try_from(envs.max_send_per_second.max(1)).unwrap_or(1);
```

### Handler Function Naming

```rust
// ✅ Good: Verb-first, concise names
pub async fn create_message(...) -> impl IntoResponse
pub async fn get_topic(...) -> impl IntoResponse
pub async fn stop_topic(...) -> impl IntoResponse
pub async fn track_open(...) -> impl IntoResponse
pub async fn handle_sns_event(...) -> impl IntoResponse

// ❌ Bad: Unnecessary suffixes
pub async fn create_message_handler(...) -> impl IntoResponse
pub async fn retrieve_topic_handler(...) -> impl IntoResponse
```

### Middleware Naming

```rust
// ✅ Good: Concise name
pub async fn api_key_auth(req: Request<Body>, next: Next) -> impl IntoResponse

// ❌ Bad: Unnecessary suffix
pub async fn api_key_auth_middleware(req: Request<Body>, next: Next) -> impl IntoResponse
```

### Constant Definitions

```rust
// ✅ Good: Group related constants at module top
const MAX_BODY_SIZE: usize = 1024 * 1024; // 1MB

/// 1x1 transparent PNG for email open tracking
const TRACKING_PIXEL: &[u8] = &[
    0x89, 0x50, 0x4E, 0x47, ...
];

// ✅ Good: Comments only when needed
const DB_MAX_CONNECTIONS: u32 = 20;
const DB_MIN_CONNECTIONS: u32 = 5;
```

### Conditional Compilation

```rust
// ✅ Good: Test-only functions
#[cfg(test)]
pub async fn save(self, db_pool: &SqlitePool) -> Result<Self, sqlx::Error> {
    // Individual save logic used only in tests
}
```

### Async Patterns

```rust
// ✅ Good: Use tokio::spawn for background tasks
tokio::spawn(async move {
    schedule_pre_send_message(&tx, db).await;
});

// ✅ Good: Use channels for inter-task communication
let (tx_send, rx_send) = tokio::sync::mpsc::channel(SEND_CHANNEL_BUFFER);

// ✅ Good: Use Arc for shared state
let tokens = Arc::new(AtomicU64::new(max_per_sec));
```

### SQL Queries

```rust
// ✅ Good: Multi-line SQL for readability
let rows: Vec<ScheduledEmailRow> = sqlx::query_as(
    "SELECT id, topic_id, email, subject, content
     FROM email_requests
     WHERE status = ? AND scheduled_at <= datetime('now')
     ORDER BY scheduled_at ASC
     LIMIT ?",
)
.bind(EmailMessageStatus::Created as i32)
.bind(BATCH_SIZE)
.fetch_all(db_pool)
.await?;

// ✅ Good: Use named columns, avoid SELECT *
```

---

## 🧪 Test Code Style Guide

### Test File Structure

```rust
#[cfg(test)]
mod tests {
    use crate::models::request::EmailRequest;
    use crate::tests::helpers::{get_api_key, setup_db};
    // ... other imports

    // Test functions
}
```

### Shared Helpers

Define shared helpers in `tests/mod.rs`:

```rust
#[cfg(test)]
pub mod helpers {
    use sqlx::{sqlite::SqlitePoolOptions, SqlitePool};

    pub async fn setup_db() -> SqlitePool {
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap();

        // Create tables
        sqlx::query("CREATE TABLE ...")
            .execute(&pool)
            .await
            .unwrap();

        pool
    }

    pub fn get_api_key() -> String {
        crate::config::get_environments().api_key.clone()
    }
}
```

### Test Function Naming

```rust
// ✅ Good: test_ prefix + target + expected result
#[tokio::test]
async fn test_save_returns_id() { }

#[tokio::test]
async fn test_sent_count_empty() { }

#[tokio::test]
async fn test_stop_topic_updates_created_only() { }

// ❌ Bad: Unclear or too long names
#[tokio::test]
async fn test1() { }

#[tokio::test]
async fn test_that_when_we_save_an_email_request_it_should_return_the_id() { }
```

### Test Helper Functions

```rust
// ✅ Good: Factory function for test data
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
```

### API Test Pattern

```rust
#[tokio::test]
async fn test_create_message_success() {
    // 1. Setup
    let db = setup_db().await;
    let (tx, _rx) = tokio::sync::mpsc::channel(100);
    let app = crate::app::app(AppState::new(db.clone(), tx));

    // 2. Prepare request
    let payload = serde_json::json!({
        "messages": [{
            "topic_id": "test",
            "emails": ["user@test.com"],
            "subject": "Hello",
            "content": "<p>Test</p>"
        }]
    });

    // 3. Execute
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

    // 4. Assert
    assert_eq!(response.status(), StatusCode::OK);
}
```

### Database Test Pattern

```rust
#[tokio::test]
async fn test_save_batch_multiple() {
    let db = setup_db().await;
    
    // Arrange
    let requests: Vec<EmailRequest> = (0..5)
        .map(|i| EmailRequest {
            id: None,
            email: format!("user{i}@example.com"),
            // ...
        })
        .collect();

    // Act
    let saved = EmailRequest::save_batch(requests, &db).await.unwrap();

    // Assert
    assert_eq!(saved.len(), 5);
    for (i, req) in saved.iter().enumerate() {
        assert_eq!(req.id, Some((i + 1) as i32));
    }

    // Verify in DB
    let count: (i32,) = sqlx::query_as("SELECT COUNT(*) FROM email_requests")
        .fetch_one(&db)
        .await
        .unwrap();
    assert_eq!(count.0, 5);
}
```

### Assertion Style

```rust
// ✅ Good: Clear assertions
assert_eq!(response.status(), StatusCode::OK);
assert_eq!(saved.id, Some(1));
assert!(counts.is_empty());

// ✅ Good: Helpful message on failure
assert_eq!(counts.get("Created"), Some(&2), "Created count mismatch");

// ❌ Bad: Unclear assertion
assert!(response.status() == StatusCode::OK);
```

### Test Categories

```
tests/
├── mod.rs              # Shared helpers
├── auth_tests.rs       # Authentication tests
├── event_tests.rs      # Event handler tests
├── handler_tests.rs    # Message/topic handler tests
├── request_tests.rs    # EmailRequest model tests
└── status_tests.rs     # EmailMessageStatus enum tests
```

### Test Isolation

```rust
// ✅ Good: Each test uses independent in-memory DB
#[tokio::test]
async fn test_independent_1() {
    let db = setup_db().await;  // Fresh in-memory DB
    // ...
}

#[tokio::test]
async fn test_independent_2() {
    let db = setup_db().await;  // Separate in-memory DB
    // ...
}
```

### Test Coverage Guidelines

1. **Model tests**: All public methods, edge cases, error conditions
2. **Handler tests**: Success paths, validation errors, auth failures
3. **Status enum tests**: All variants, conversion functions
4. **Integration tests**: Full request/response cycles

---

## 🔄 Common Modification Patterns

### Adding a New API Endpoint

1. Create handler function in `src/handlers/`
2. Add route in `src/app.rs`
3. Add authentication if needed (check `middlewares/auth_middlewares.rs`)
4. Write tests in `src/tests/`

### Adding a New Database Column

1. Update `init_database.sh` schema
2. Update model in `src/models/`
3. Update test helper `setup_db()` in `src/tests/mod.rs`
4. Update relevant queries

### Modifying Rate Limiting

1. Edit constants in `src/services/receiver.rs`
2. Or change `MAX_SEND_PER_SECOND` environment variable

### Adding a New Background Task

1. Create task function in `src/services/`
2. Create channel if needed in `main.rs`
3. Spawn task with `tokio::spawn` in `main.rs`

---

## 🚨 Known Limitations

1. **Emails per request**: Maximum 10,000
2. **Rate Limiting**: Controlled by `MAX_SEND_PER_SECOND` env var
3. **Database**: SQLite single file (not suitable for horizontal scaling)
4. **Scheduler**: Single instance (no distributed locking)
5. **Timezone**: `scheduled_at` is parsed as local time, stored as UTC

---

## 🤝 Contribution Guidelines

1. **Branch naming**: `feature/feature-name`, `fix/bug-name`
2. **Commit messages**: `[module] Summary of changes`
3. **Tests**: All `cargo test` must pass
4. **Clippy**: No warnings with `cargo clippy`
5. **Formatting**: Apply `cargo fmt` before commit

### Pre-commit Checklist

```bash
cargo fmt
cargo clippy
cargo test
```

---

## 📚 Quick Reference

### Status Flow

```
Created (0) ──┬──▶ Processed (1) ──▶ Sent (2)
              │                  └──▶ Failed (3)
              └──▶ Stopped (4)
```

### Channel Flow

```
API Handler ──▶ tx_send ──▶ Sender ──▶ tx_post_send ──▶ Post-Processor
     │              ▲
     │              │
     └── Scheduler ─┘
```

### Key Files for Common Tasks

| Task | Primary Files |
|------|---------------|
| Add endpoint | `handlers/*.rs`, `app.rs` |
| Modify sending | `services/receiver.rs`, `services/sender.rs` |
| Change schema | `init_database.sh`, `models/*.rs`, `tests/mod.rs` |
| Update config | `config.rs`, `.env` |
| Add tests | `tests/*.rs` |

---

*Last updated: 2025-12-27*
