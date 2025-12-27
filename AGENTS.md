# AGENTS.md

> Project Guide for AI Coding Agents

---

## 📋 Project Overview

**aws-ses-sender** is a high-performance bulk email sending service via AWS SES.

### Key Features
- 🚀 **Bulk Email Sending**: Process up to 10,000 emails per request
- ⏰ **Scheduled Sending**: Schedule future deliveries via `scheduled_at` field
- 📊 **Event Tracking**: Receive Bounce/Complaint/Delivery events via AWS SNS
- 👀 **Open Tracking**: Track email opens via 1x1 transparent pixel
- ⚡ **Rate Limiting**: Token Bucket + Semaphore-based sends per second control

### Tech Stack
| Area | Technology |
|------|------|
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
1. **Immediate Sending**: API → Batch INSERT → Send channel → Rate-limited sending → Batch result update
2. **Scheduled Sending**: API → Batch INSERT (Created) → Scheduler polling → Send channel → Sending → Result update

---

## 📁 Project Structure

```
src/
├── main.rs                 # Entry point, initialization, background task spawning
├── app.rs                  # Axum router setup
├── config.rs               # Environment variable loading (singleton)
├── state.rs                # AppState definition (DB pool, channels)
├── handlers/               # HTTP request handlers
│   ├── mod.rs
│   ├── message_handlers.rs # POST /v1/messages
│   ├── event_handlers.rs   # GET/POST /v1/events/*
│   └── topic_handlers.rs   # GET/DELETE /v1/topics/{id}
├── services/               # Background services
│   ├── mod.rs
│   ├── scheduler.rs        # Scheduled email polling (10-second interval)
│   ├── receiver.rs         # Rate-limited sending + batch DB updates
│   └── sender.rs           # AWS SES API calls (singleton client)
├── models/                 # Data models
│   ├── mod.rs
│   ├── content.rs          # EmailContent (subject, content storage)
│   ├── request.rs          # EmailRequest, EmailMessageStatus
│   └── result.rs           # EmailResult
├── middlewares/            # HTTP middlewares
│   ├── mod.rs
│   └── auth_middlewares.rs # API Key authentication
└── tests/                  # Tests
    ├── mod.rs              # Shared helper functions
    ├── auth_tests.rs
    ├── event_tests.rs
    ├── handler_tests.rs
    ├── request_tests.rs
    ├── scheduler_tests.rs
    ├── status_tests.rs
    └── topic_tests.rs
```

---

## 🔑 Core Modules

### `src/main.rs`
- Application entry point
- Logger, Sentry, DB initialization
- Spawns 3 background tasks

### `src/services/receiver.rs`
**Most complex module** - Handles rate limiting and concurrency control

```rust
// Token Bucket: Controls sends per second
let tokens = Arc::new(AtomicU64::new(max_per_sec));

// Semaphore: Limits concurrent requests (max_per_sec * 2)
let semaphore = Arc::new(Semaphore::new(max_per_sec * 2));
```

### `src/models/request.rs`
```rust
pub enum EmailMessageStatus {
    Created = 0,    // Created (waiting for scheduled send)
    Processed = 1,  // Processed (queued for sending)
    Sent = 2,       // Sent successfully
    Failed = 3,     // Send failed
    Stopped = 4,    // Sending stopped
}
```

---

## 🗄 Database Schema

### `email_contents` Table
| Column | Type | Description |
|------|------|------|
| id | INTEGER PK | Auto-increment ID |
| subject | VARCHAR(255) | Subject |
| content | TEXT | HTML body |
| created_at | DATETIME | Creation time |

> **Note**: Subject and content are stored in a separate table to prevent duplication. Improves storage efficiency when sending the same content to multiple recipients.

### `email_requests` Table
| Column | Type | Description |
|------|------|------|
| id | INTEGER PK | Auto-increment ID |
| topic_id | VARCHAR(255) | Group sending identifier |
| content_id | INTEGER FK | References email_contents.id |
| message_id | VARCHAR(255) | AWS SES message ID |
| email | VARCHAR(255) | Recipient email |
| scheduled_at | DATETIME | Scheduled send time |
| status | TINYINT | EmailMessageStatus value |
| error | VARCHAR(255) | Error message |
| created_at | DATETIME | Creation time |
| updated_at | DATETIME | Update time |

### `email_results` Table
| Column | Type | Description |
|------|------|------|
| id | INTEGER PK | Auto-increment ID |
| request_id | INTEGER FK | References email_requests.id |
| status | VARCHAR(50) | Event type |
| raw | TEXT | Raw SNS JSON |
| created_at | DATETIME | Creation time |

---

## 🌐 API Endpoints

| Method | Path | Auth | Handler Function |
|--------|------|------|-------------|
| POST | `/v1/messages` | ✅ | `create_message` |
| GET | `/v1/topics/{topic_id}` | ✅ | `get_topic` |
| DELETE | `/v1/topics/{topic_id}` | ✅ | `stop_topic` |
| GET | `/v1/events/open` | ❌ | `track_open` |
| GET | `/v1/events/counts/sent` | ✅ | `get_sent_count` |
| POST | `/v1/events/results` | ❌ | `handle_sns_event` |

---

## ⚙️ Environment Variables

| Variable | Required | Default | Description |
|------|------|--------|------|
| `SERVER_PORT` | ❌ | 8080 | Server port |
| `SERVER_URL` | ✅ | - | External access URL |
| `API_KEY` | ✅ | - | API authentication key |
| `AWS_REGION` | ❌ | ap-northeast-2 | AWS region |
| `AWS_SES_FROM_EMAIL` | ✅ | - | Sender email |
| `MAX_SEND_PER_SECOND` | ❌ | 24 | Maximum sends per second |
| `SENTRY_DSN` | ❌ | - | Sentry DSN |
| `RUST_LOG` | ❌ | info | Log level |

---

## 🔧 Development Environment

### Build and Run

```bash
# Development mode
cargo run

# Release mode
cargo run --release

# Test
cargo test

# Linting
cargo clippy
cargo fmt
```

### Key Constants

| Constant | Value | Location |
|------|-----|------|
| `DB_MAX_CONNECTIONS` | 20 | main.rs |
| `SEND_CHANNEL_BUFFER` | 10,000 | main.rs |
| `BATCH_SIZE` (scheduler) | 1,000 | scheduler.rs |
| `BATCH_INSERT_SIZE` | 100 | content.rs, request.rs |
| `BATCH_FLUSH_INTERVAL_MS` | 500 | receiver.rs |

---

## 📝 Rust Code Style Guide

> This project follows the [Rust Official Style Guide](https://doc.rust-lang.org/stable/style-guide/) and [Rust API Guidelines](https://rust-lang.github.io/api-guidelines/).

### Lint Settings

```toml
[lints.rust]
unsafe_code = "forbid"

[lints.clippy]
all = "warn"
pedantic = "warn"
nursery = "warn"
```

### Naming Conventions

| Item | Style | Example |
|------|--------|------|
| Crates/Modules | `snake_case` | `email_sender`, `auth_middlewares` |
| Types/Traits | `PascalCase` | `EmailRequest`, `SendEmailError` |
| Functions/Methods | `snake_case` | `send_email`, `get_topic` |
| Constants | `SCREAMING_SNAKE_CASE` | `MAX_EMAILS_PER_REQUEST`, `BATCH_SIZE` |
| Variables/Parameters | `snake_case` | `db_pool`, `topic_id` |
| Lifetimes | Short lowercase | `'a`, `'de` |
| Type Parameters | Single uppercase or `PascalCase` | `T`, `E`, `Item` |

### Module Documentation Comments

```rust
// ✅ Good: Concise one-liner
//! Email request model and database operations

// ❌ Bad: Unnecessarily long and verbose
//! This module handles email request models and database operations.
//! 
//! ## Key Features
//! - Save email requests
//! - Retrieve email requests
//! ...
```

### Function Documentation Comments

```rust
// ✅ Good: Concise when necessary
/// Saves multiple requests in a single transaction using multi-row INSERT.
///
/// This provides ~10x performance improvement over individual inserts.
pub async fn save_batch(requests: Vec<Self>, db_pool: &SqlitePool) -> Result<Vec<Self>, sqlx::Error>

// ✅ Good: Simple functions can omit documentation
pub async fn update(&self, db_pool: &SqlitePool) -> Result<(), sqlx::Error>

// ❌ Bad: Repeating what's obvious from the code
/// This function updates the email request in the database
/// It takes a database pool and updates the request
pub async fn update(&self, db_pool: &SqlitePool) -> Result<(), sqlx::Error>
```

### No Separator Comments

```rust
// ❌ Bad: Using separator comments
// =============================================================================
// Configuration
// =============================================================================
const BATCH_SIZE: usize = 100;

// ✅ Good: Group related constants (separated by blank lines)
// Token bucket configuration
const TOKEN_REFILL_INTERVAL_MS: u64 = 100;
const TOKEN_WAIT_INTERVAL_MS: u64 = 5;

// Batch update configuration
const BATCH_SIZE: usize = 100;
const BATCH_FLUSH_INTERVAL_MS: u64 = 500;
```

### Import Organization

```rust
// ✅ Good: Standard library → External crates → Internal modules order
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
// ✅ Good: Use thiserror
#[derive(Debug, Error)]
pub enum SendEmailError {
    #[error("Failed to build email: {0}")]
    Build(String),

    #[error("SES SDK error: {0}")]
    Sdk(String),
}

// ✅ Good: Use let-else pattern
let Some(ses_msg_id) = ses_msg_id else {
    error!("SES message_id not found");
    return (StatusCode::BAD_REQUEST, "Not found").into_response();
};

// ✅ Good: Use ? operator
let row: (i64,) = sqlx::query_as("SELECT id FROM ...")
    .bind(message_id)
    .fetch_one(db_pool)
    .await?;
```

### Conditional Compilation

```rust
// ✅ Good: Test-only functions
#[cfg(test)]
pub async fn save(self, db_pool: &SqlitePool) -> Result<Self, sqlx::Error> {
    // Individual save logic used only in tests
}
```

### Type Conversion

```rust
// ✅ Good: Explicit casting with allow attribute
#[allow(clippy::cast_possible_truncation)]
let id = row.0 as i32;

// ✅ Good: Safe conversion
let max_per_sec = u64::try_from(envs.max_send_per_second.max(1)).unwrap_or(1);
```

### Handler Function Naming

```rust
// ✅ Good: Concise names starting with verbs
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

// ✅ Good: Comments only when necessary
const DB_MAX_CONNECTIONS: u32 = 20;
const DB_MIN_CONNECTIONS: u32 = 5;
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

### Shared Helper Functions

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
// ✅ Good: test_ prefix + test target + expected result
#[tokio::test]
async fn test_save_returns_id() { }

#[tokio::test]
async fn test_sent_count_empty() { }

#[tokio::test]
async fn test_stop_topic_updates_created_only() { }

// ❌ Bad: Unclear or overly long names
#[tokio::test]
async fn test1() { }

#[tokio::test]
async fn test_that_when_we_save_an_email_request_it_should_return_the_id() { }
```

### Test Helper Functions

```rust
// ✅ Good: Content creation helper
fn create_test_content() -> EmailContent {
    EmailContent {
        id: None,
        subject: "Test Subject".to_string(),
        content: "<p>Test Content</p>".to_string(),
    }
}

// ✅ Good: Create request with content_id
fn create_test_request_with_content_id(content_id: i32) -> EmailRequest {
    EmailRequest {
        id: None,
        topic_id: Some("test_topic".to_string()),
        content_id: Some(content_id),
        email: "test@example.com".to_string(),
        subject: String::new(),  // Loaded via JOIN at runtime
        content: String::new(),  // Loaded via JOIN at runtime
        scheduled_at: None,
        status: EmailMessageStatus::Created as i32,
        error: None,
        message_id: None,
    }
}

// ✅ Good: Save content to DB then create request
async fn create_test_request_with_db(db: &SqlitePool) -> EmailRequest {
    let content = create_test_content().save(db).await.unwrap();
    create_test_request_with_content_id(content.id.unwrap())
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

// ✅ Good: Useful message on failure
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
├── scheduler_tests.rs  # Scheduler tests
├── status_tests.rs     # EmailMessageStatus enum tests
└── topic_tests.rs      # Topic handler tests
```

### Test Isolation

```rust
// ✅ Good: Each test uses an independent in-memory DB
#[tokio::test]
async fn test_independent_1() {
    let db = setup_db().await;  // New in-memory DB
    // ...
}

#[tokio::test]
async fn test_independent_2() {
    let db = setup_db().await;  // Separate in-memory DB
    // ...
}
```

---

## 🚨 Known Limitations

1. **Emails per request**: Maximum 10,000
2. **Rate Limiting**: Controlled by `MAX_SEND_PER_SECOND` environment variable
3. **DB Size**: Single SQLite file
4. **Concurrency**: Single scheduler instance

---

## 🤝 Contribution Guidelines

1. **Branch Naming**: `feature/feature-name`, `fix/bug-name`
2. **Commit Messages**: `[module-name] Summary of changes`
3. **Tests Pass**: All `cargo test` must pass
4. **Clippy Pass**: No `cargo clippy` warnings
5. **Code Formatting**: Apply `cargo fmt`

---

*Last Updated: 2025-12-27*
