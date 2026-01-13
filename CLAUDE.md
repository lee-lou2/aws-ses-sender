# CLAUDE.md

## Project Overview

AWS SES Sender is a high-performance bulk email service built with Rust. It provides a REST API for sending emails via AWS Simple Email Service (SES) with features like scheduling, rate limiting, delivery tracking, and batch processing.

## Quick Start

```bash
# Build the project
cargo build --release

# Run tests
cargo test

# Run the server (requires .env configuration)
cargo run --release

# Format code
cargo fmt

# Run linter
cargo clippy
```

## Project Structure

```
/home/user/aws-ses-sender/
├── src/
│   ├── main.rs                   # Entry point, server initialization
│   ├── app.rs                    # HTTP routing configuration
│   ├── error.rs                  # Centralized error handling (AppError)
│   ├── state.rs                  # Shared application state (AppState)
│   ├── constants.rs              # Global constants (BATCH_SIZE, MAX_RETRIES)
│   ├── config/
│   │   ├── mod.rs                # Module exports
│   │   ├── env.rs                # Environment variable management (AppConfig)
│   │   └── db.rs                 # Database initialization and migrations
│   ├── models/
│   │   ├── mod.rs                # Module exports
│   │   ├── request.rs            # EmailRequest model + DB operations
│   │   ├── result.rs             # EmailResult model + delivery tracking
│   │   └── content.rs            # EmailContent model (deduplication)
│   ├── handlers/
│   │   ├── mod.rs                # Module exports
│   │   ├── message_handlers.rs   # POST /v1/messages - create/send emails
│   │   ├── topic_handlers.rs     # GET/DELETE /v1/topics - topic management
│   │   ├── event_handlers.rs     # SNS webhooks + open/click tracking
│   │   └── health_handlers.rs    # Health check endpoints
│   ├── middlewares/
│   │   ├── mod.rs                # Module exports
│   │   └── auth_middlewares.rs   # X-API-KEY authentication
│   ├── services/
│   │   ├── mod.rs                # Module exports
│   │   ├── sender.rs             # AWS SES sending + rate limiting
│   │   ├── scheduler.rs          # Scheduled email pickup
│   │   └── receiver.rs           # Post-processing + batch result persistence
│   └── tests/
│       ├── mod.rs                # Test utilities and helpers
│       ├── handler_tests.rs      # API endpoint tests
│       ├── auth_tests.rs         # Authentication tests
│       ├── event_tests.rs        # SNS event handling tests
│       ├── health_tests.rs       # Health endpoint tests
│       ├── request_tests.rs      # EmailRequest model tests
│       ├── scheduler_tests.rs    # Scheduler service tests
│       ├── status_tests.rs       # Status enum tests
│       └── topic_tests.rs        # Topic handler tests
├── migrations/
│   └── 20241228000000_initial_schema.sql  # Database schema
├── Cargo.toml                    # Dependencies and project config
├── rustfmt.toml                  # Code formatting rules
├── Dockerfile                    # Container build configuration
└── docs/                         # Additional documentation
```

## Architecture

### Multi-Stage Pipeline

The application implements a producer-consumer pipeline with 3 async stages:

```
┌─────────────┐     ┌───────────────┐     ┌────────────────┐     ┌──────────┐
│  HTTP API   │────►│ Email Sender  │────►│ Post-Processor │────►│ Database │
│  (Handler)  │     │ (Rate Limited)│     │ (Batch Update) │     │ (SQLite) │
└─────────────┘     └───────────────┘     └────────────────┘     └──────────┘
       │                    ▲                     │
       │    mpsc channel    │     mpsc channel    │
       └────────────────────┘─────────────────────┘
```

1. **HTTP Handlers**: Receive requests, validate, save to DB, send to channel
2. **Email Sender**: Rate-limited sender with token bucket + concurrent processing
3. **Post-Processor**: Batch accumulates results and persists to DB

### Key Design Patterns

- **State Extractor**: Axum's `State<AppState>` for dependency injection
- **Centralized Error Type**: `AppError` enum implementing `IntoResponse`
- **Resource Pooling**: SQLite connection pool with configurable limits
- **Arc String Sharing**: Memory optimization for duplicate content
- **Batch Operations**: Multi-row INSERT/UPDATE for 10-100x performance
- **Token Bucket Rate Limiting**: Event-driven refill with atomic operations
- **Lazy Static Configuration**: `LazyLock<AppConfig>` for thread-safe init

## Technology Stack

### Core Dependencies

| Category | Crate | Version | Purpose |
|----------|-------|---------|---------|
| Web Framework | axum | 0.8 | HTTP routing and handlers |
| Async Runtime | tokio | 1.43 | Full-featured async runtime |
| AWS SDK | aws-sdk-sesv2 | 1.65 | AWS SES v2 email sending |
| Database | sqlx | 0.8 | Compile-time checked SQL queries |
| Serialization | serde | 1.0 | JSON serialization |
| Date/Time | chrono | 0.4 | Timezone-aware date handling |
| Logging | tracing | 0.1 | Structured logging |
| Error Tracking | sentry | 0.36 | Error monitoring |
| Error Definition | thiserror | 2.0 | Error enum macro |
| Memory Allocator | mimalloc | 0.1 | High-performance allocator |

### Database

- **SQLite** with WAL mode for high-performance concurrent reads/writes
- Connection pooling via sqlx
- Compile-time query verification

## Code Style Guide

### Formatting Rules (rustfmt.toml)

```toml
max_width = 100                    # Maximum line width
tab_spaces = 4                     # 4 spaces for indentation
newline_style = "Unix"             # LF line endings
imports_granularity = "Module"     # Group imports by module
group_imports = "StdExternalCrate" # Order: std → external → crate
fn_params_layout = "Tall"          # One parameter per line for long lists
wrap_comments = true               # Wrap comments at 100 chars
```

### Clippy Lints

The project uses strict Clippy settings:

```rust
#![warn(clippy::all, clippy::pedantic, clippy::nursery)]
#![forbid(unsafe_code)]
```

**Allowed lints:**
- `clippy::module_name_repetitions` - Allow types like `EmailRequest` in `request.rs`
- `clippy::too_many_lines` - Allow complex handler functions
- `clippy::significant_drop_tightening` - Allow natural drop patterns

## Naming Conventions

### General Rules

| Category | Convention | Examples |
|----------|------------|----------|
| Modules | snake_case | `message_handlers.rs`, `auth_middlewares.rs` |
| Types/Structs | PascalCase | `EmailRequest`, `TokenBucket`, `AppError` |
| Functions | snake_case | `send_email`, `get_topic`, `fetch_batch` |
| Constants | SCREAMING_SNAKE_CASE | `MAX_RETRIES`, `BATCH_SIZE`, `API_KEY_HEADER` |
| Enums | PascalCase (type and variants) | `EmailMessageStatus::Sent` |
| Fields | snake_case | `topic_id`, `message_id`, `created_at` |
| Test functions | `test_<feature>_<scenario>` | `test_create_message_success` |

### Specific Patterns

```rust
// Constants
pub const MAX_RETRIES: u32 = 3;
pub const BATCH_SIZE: usize = 1000;
pub const API_KEY_HEADER: &str = "X-API-KEY";

// Type definitions
pub struct EmailRequest { ... }
pub enum EmailMessageStatus { ... }

// Functions
pub async fn send_email(request: &EmailRequest) -> Result<String, SendEmailError>
pub fn get_topic(topic_id: &str) -> AppResult<TopicResponse>

// Test functions
#[tokio::test]
async fn test_create_message_success() { ... }
async fn test_token_bucket_refill() { ... }
```

## Error Handling

### Centralized AppError

All errors are handled through the `AppError` enum in `src/error.rs`:

```rust
#[derive(Error, Debug)]
pub enum AppError {
    #[error("Bad request: {0}")]
    BadRequest(String),

    #[error("Unauthorized: {0}")]
    Unauthorized(String),

    #[error("Not found: {0}")]
    NotFound(String),

    #[error("Validation error: {0}")]
    Validation(String),

    #[error("Internal server error: {0}")]
    Internal(String),

    #[error("Database error: {0}")]
    Database(#[from] sqlx::Error),

    #[error("Email error: {0}")]
    Email(String),

    #[error("Channel closed")]
    ChannelClosed,
}
```

### HTTP Status Mapping

| Error Variant | HTTP Status |
|---------------|-------------|
| `BadRequest` | 400 |
| `Unauthorized` | 401 |
| `NotFound` | 404 |
| `Validation` | 422 |
| `Internal` | 500 |
| `Database` | 500 |
| `Email` | 500 |
| `ChannelClosed` | 500 |

### Usage Pattern

```rust
pub async fn handler(
    State(state): State<AppState>,
    Json(payload): Json<RequestPayload>,
) -> AppResult<impl IntoResponse> {
    // Validation errors
    if payload.data.is_empty() {
        return Err(AppError::BadRequest("Data is required".to_string()));
    }

    // Database errors (auto-converted via #[from])
    let result = Model::find(&state.db_pool, id).await?;

    // Not found errors
    let item = result.ok_or_else(|| AppError::NotFound("Item not found".to_string()))?;

    Ok(Json(item))
}
```

## Comment Style

### Documentation Comments

Use `///` for public API documentation:

```rust
/// Creates a new `AppState` instance.
///
/// # Arguments
/// * `db_pool` - SQLite connection pool
/// * `tx` - Channel sender for email requests
#[must_use]
pub const fn new(db_pool: SqlitePool, tx: mpsc::Sender<EmailRequest>) -> Self {
    Self { db_pool, tx }
}
```

### Module-Level Documentation

Use `//!` at the top of module files:

```rust
//! High-performance bulk email service via AWS SES.
//!
//! This module handles email sending, scheduling, and delivery tracking.
```

### Inline Comments

Use `//` for implementation details:

```rust
// Phase 1: Atomically update and return basic info (no subqueries)
let updated: Vec<UpdatedRow> = sqlx::query_as(...)

// Use Arc to share subject/content across all emails in the same message,
// avoiding expensive string cloning (e.g., 10,000 emails = 1 Arc::clone vs 10,000 String::clone)
let subject = Arc::new(saved_content.subject.clone());
```

### When to Comment

1. **Complex algorithms**: Explain the why, not the what
2. **Performance optimizations**: Document why a less obvious approach is used
3. **Business logic**: Explain domain-specific rules
4. **Workarounds**: Document any temporary fixes or hacks
5. **TODOs**: Use `// TODO:` for future improvements

### When NOT to Comment

1. **Self-explanatory code**: Don't comment obvious operations
2. **Type information**: Let the type system speak for itself
3. **Commit messages**: Don't duplicate git history in comments

## Testing Guide

### Test Structure

Tests are organized inline within source files under `#[cfg(test)]` modules:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::tests::helpers::*;

    #[tokio::test]
    async fn test_feature_scenario() {
        // Arrange
        let db = setup_db().await;
        let (tx, _rx) = tokio::sync::mpsc::channel(100);

        // Act
        let result = function_under_test(&db).await;

        // Assert
        assert!(result.is_ok());
    }
}
```

### Test Helpers (src/tests/mod.rs)

```rust
// Database setup - creates in-memory SQLite with schema
pub async fn setup_db() -> SqlitePool

// Test data factories
pub fn create_test_content() -> EmailContent
pub fn create_test_request_with_content_id(content_id: i32) -> EmailRequest

// Database insertion helpers
pub async fn insert_default_content(db: &SqlitePool) -> i64
pub async fn insert_request_raw(db: &SqlitePool, ...) -> i64
pub async fn insert_request_with_id(db: &SqlitePool, id: i32, ...) -> i64
```

### Integration Test Pattern

```rust
#[tokio::test]
async fn test_api_endpoint() {
    // Setup
    let db = setup_db().await;
    let (tx, _rx) = tokio::sync::mpsc::channel(100);
    let app = crate::app::app(AppState::new(db.clone(), tx));

    // Create request
    let payload = serde_json::json!({
        "field": "value"
    });

    // Execute
    let response = app.oneshot(
        Request::builder()
            .uri("/v1/endpoint")
            .method("POST")
            .header("Content-Type", "application/json")
            .header("X-API-KEY", get_api_key())
            .body(Body::from(serde_json::to_string(&payload).unwrap()))
            .unwrap()
    ).await.unwrap();

    // Verify HTTP response
    assert_eq!(response.status(), StatusCode::OK);

    // Verify database state
    let count: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM table")
        .fetch_one(&db)
        .await
        .unwrap();
    assert_eq!(count.0, 1);
}
```

### Test Categories

1. **HTTP API Tests**: Request/response validation, status codes, authentication
2. **Model Tests**: Serialization, database CRUD operations, batch inserts
3. **Error Handling Tests**: All error variants, HTTP status mapping
4. **Configuration Tests**: Environment variables, default values
5. **Service Tests**: Rate limiting, token bucket, batch updates
6. **Edge Cases**: Unicode, special characters, boundary conditions

### Running Tests

```bash
# Run all tests
cargo test

# Run tests with output
cargo test -- --nocapture

# Run specific test
cargo test test_create_message_success

# Run tests in a specific module
cargo test handler_tests

# Run tests with verbose output
cargo test -- --show-output
```

## Database Schema

### Tables

```sql
-- Stores unique email content (deduplication)
CREATE TABLE email_contents (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    subject VARCHAR(255) NOT NULL,
    content TEXT NOT NULL,
    created_at DATETIME NOT NULL DEFAULT (datetime('now'))
);

-- Main email request table
CREATE TABLE email_requests (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    topic_id VARCHAR(255) NOT NULL,
    content_id INTEGER NOT NULL,          -- FK to email_contents
    message_id VARCHAR(255) DEFAULT NULL, -- SES message ID after sending
    email VARCHAR(255) NOT NULL,
    scheduled_at DATETIME NOT NULL,
    status TINYINT NOT NULL DEFAULT 0,    -- EmailMessageStatus enum
    error VARCHAR(255) DEFAULT NULL,
    created_at DATETIME NOT NULL DEFAULT (datetime('now')),
    updated_at DATETIME NOT NULL DEFAULT (datetime('now')),
    deleted_at DATETIME,
    FOREIGN KEY (content_id) REFERENCES email_contents(id)
);

-- Delivery tracking (SNS events)
CREATE TABLE email_results (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    request_id INTEGER NOT NULL,
    status VARCHAR(50) NOT NULL,          -- Bounce, Complaint, Delivery, Open, etc.
    raw TEXT,                             -- Full SNS event JSON
    created_at DATETIME NOT NULL DEFAULT (datetime('now')),
    FOREIGN KEY (request_id) REFERENCES email_requests(id)
);
```

### Email Status Enum

```rust
pub enum EmailMessageStatus {
    Created = 0,    // Initial state, awaiting scheduling
    Processed = 1,  // Picked up by scheduler, ready for sending
    Sent = 2,       // Successfully sent to SES
    Failed = 3,     // Failed to send
    Stopped = 4,    // Explicitly stopped before sending
}
```

### Key Indexes

| Index | Columns | Purpose |
|-------|---------|---------|
| `idx_requests_topic_id` | topic_id | Topic lookups and statistics |
| `idx_requests_content_id` | content_id | Content JOIN operations |
| `idx_requests_message_id` | message_id | SES result lookups |
| `idx_requests_status_scheduled` | status, scheduled_at | Scheduler queries |
| `idx_requests_status_topic` | status, topic_id | Topic filtering |
| `idx_results_request_id` | request_id | Event lookups |

## API Endpoints

### Email Operations

| Method | Endpoint | Description |
|--------|----------|-------------|
| POST | `/v1/messages` | Create and schedule emails |
| GET | `/v1/topics/:topic_id` | Get topic statistics |
| DELETE | `/v1/topics/:topic_id` | Stop all pending emails for topic |

### Event Tracking

| Method | Endpoint | Description |
|--------|----------|-------------|
| POST | `/v1/events/sns` | AWS SNS webhook for delivery events |
| GET | `/v1/events/open` | Track email opens (pixel) |
| GET | `/v1/events/click` | Track link clicks |

### Health Check

| Method | Endpoint | Description |
|--------|----------|-------------|
| GET | `/health` | Basic health check |
| GET | `/health/detail` | Detailed health with DB stats |

## Configuration

### Environment Variables

| Variable | Default | Description |
|----------|---------|-------------|
| `SERVER_PORT` | 8080 | HTTP server port |
| `SERVER_URL` | - | Public server URL (for tracking links) |
| `API_KEY` | - | API authentication key |
| `AWS_REGION` | ap-northeast-2 | AWS region for SES |
| `AWS_SES_FROM_EMAIL` | - | Sender email address |
| `MAX_SEND_PER_SECOND` | 24 | Rate limit for SES sends |
| `DB_MAX_CONNECTIONS` | 20 | Max SQLite connections |
| `DB_MIN_CONNECTIONS` | 5 | Min SQLite connections |
| `SEND_CHANNEL_BUFFER` | 10000 | Sender channel buffer size |
| `POST_SEND_CHANNEL_BUFFER` | 1000 | Post-processor channel buffer |
| `SENTRY_DSN` | - | Sentry error tracking DSN |
| `SENTRY_TRACES_SAMPLE_RATE` | 0.1 | Sentry trace sampling rate |

## Performance Optimizations

1. **Batch Inserts**: Multi-row INSERT for 10-100x faster writes
2. **Arc String Sharing**: Share content across emails to reduce memory
3. **WAL Mode**: SQLite write-ahead logging for concurrent access
4. **Connection Pooling**: Reuse database connections
5. **Token Bucket**: Efficient rate limiting with atomic operations
6. **mimalloc**: High-performance memory allocator
7. **Async Pipeline**: Non-blocking message passing between stages

## Common Tasks

### Adding a New Handler

1. Create handler function in appropriate file under `src/handlers/`
2. Add route in `src/app.rs`
3. Write tests in `src/tests/`

```rust
// src/handlers/new_handlers.rs
pub async fn new_handler(
    State(state): State<AppState>,
    Json(payload): Json<NewRequest>,
) -> AppResult<impl IntoResponse> {
    // Implementation
    Ok(Json(response))
}

// src/app.rs
.route("/v1/new", post(new_handler))
```

### Adding a New Model

1. Create model file in `src/models/`
2. Define struct with serde derives
3. Implement database operations
4. Add to `src/models/mod.rs` exports

```rust
// src/models/new_model.rs
use serde::{Deserialize, Serialize};
use sqlx::FromRow;

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct NewModel {
    pub id: i32,
    pub field: String,
}

impl NewModel {
    pub async fn find(db: &SqlitePool, id: i32) -> Result<Option<Self>, sqlx::Error> {
        sqlx::query_as("SELECT * FROM table WHERE id = ?")
            .bind(id)
            .fetch_optional(db)
            .await
    }
}
```

### Adding Database Migrations

1. Create new SQL file in `migrations/` with timestamp prefix
2. Follow naming: `YYYYMMDDHHMMSS_description.sql`
3. Migrations run automatically on startup

```sql
-- migrations/20250113000000_add_new_table.sql
CREATE TABLE new_table (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    field VARCHAR(255) NOT NULL,
    created_at DATETIME NOT NULL DEFAULT (datetime('now'))
);

CREATE INDEX idx_new_table_field ON new_table(field);
```

## Debugging Tips

1. **Enable trace logging**: Set `RUST_LOG=debug` or `RUST_LOG=trace`
2. **Database queries**: SQLx logs queries at debug level
3. **Channel issues**: Check buffer sizes and receiver status
4. **Rate limiting**: Monitor token bucket refill in sender.rs
5. **Memory issues**: Profile with `cargo build --release` and valgrind

## Security Considerations

1. **API Key Authentication**: All endpoints require valid X-API-KEY header
2. **No Unsafe Code**: `#![forbid(unsafe_code)]` enforced project-wide
3. **SQL Injection Prevention**: All queries use parameterized bindings
4. **Input Validation**: Request payloads validated before processing
5. **Error Sanitization**: Internal errors not exposed in API responses
