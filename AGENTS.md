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
- 🔐 **API Key Authentication**: Secure API access via X-API-KEY header
- 📊 **Sentry Integration**: Real-time error tracking and monitoring
- 🚀 **High-Performance Allocator**: Uses mimalloc

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
| Memory Allocator | mimalloc |

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
1. **Immediate Sending**: API → Batch INSERT → `try_send()` to channel → Rate-limited sending → CASE WHEN batch update
2. **Scheduled Sending**: API → Batch INSERT (Created) → Two-phase scheduler (UPDATE...RETURNING + JOIN) → Send channel → Sending → Result update

---

## 📁 Project Structure

```
├── migrations/             # SQLx database migrations (auto-applied on startup)
│   └── 20241228000000_initial_schema.sql
├── src/
│   ├── main.rs                 # Entry point, mimalloc, graceful shutdown
│   ├── app.rs                  # Axum router setup
│   ├── error.rs                # Centralized error handling (AppError, AppResult)
│   ├── constants.rs            # Shared constants (BATCH_INSERT_SIZE)
│   ├── state.rs                # AppState definition (DB pool, channels)
│   ├── config/                 # Configuration module
│   │   ├── mod.rs              # Module exports
│   │   ├── env.rs              # Environment variable loading (AppConfig)
│   │   └── db.rs               # Database connection pool management
│   ├── handlers/               # HTTP request handlers
│   │   ├── mod.rs
│   │   ├── message_handlers.rs # POST /v1/messages
│   │   ├── event_handlers.rs   # GET/POST /v1/events/*
│   │   ├── health_handlers.rs  # GET /health, /ready
│   │   └── topic_handlers.rs   # GET/DELETE /v1/topics/{id}
│   ├── services/               # Background services
│   │   ├── mod.rs
│   │   ├── scheduler.rs        # Scheduled email polling (10-second interval)
│   │   ├── receiver.rs         # Rate-limited sending + batch DB updates
│   │   └── sender.rs           # AWS SES API calls (singleton client, retry logic)
│   ├── models/                 # Data models
│   │   ├── mod.rs
│   │   ├── content.rs          # EmailContent (subject, content storage)
│   │   ├── request.rs          # EmailRequest, EmailMessageStatus
│   │   └── result.rs           # EmailResult
│   └── middlewares/            # HTTP middlewares
│       ├── mod.rs
│       └── auth_middlewares.rs # API Key authentication
└── Cargo.toml
```

---

## 🔑 Core Modules

### `src/main.rs`
- Application entry point
- mimalloc global allocator setup (non-MSVC targets)
- Logger, Sentry initialization
- SQLx migrations auto-applied via config/db.rs
- Spawns 3 background tasks
- Graceful shutdown with `tokio::signal::ctrl_c()`

### `src/error.rs`
**Centralized Error Handling** - All errors are converted to `AppError`

```rust
#[derive(Error, Debug)]
pub enum AppError {
    BadRequest(String),      // 400
    Unauthorized(String),    // 401
    NotFound(String),        // 404
    Validation(String),      // 400
    Internal(String),        // 500
    Database(#[from] sqlx::Error),
    Email(String),
    ChannelClosed,
}

pub type AppResult<T> = Result<T, AppError>;
```

### `src/config/`
**Configuration Module** - Environment variables and database management

```rust
// env.rs - Environment variable helpers
pub fn get_env(key: &str, default: Option<&str>) -> String;
pub fn get_env_parsed<T: FromStr>(key: &str, default: T) -> T;

// AppConfig - All settings in one struct
pub struct AppConfig {
    pub server_port: String,
    pub max_send_per_second: i32,
    pub db_max_connections: u32,
    // ...
}

pub static APP_CONFIG: Lazy<AppConfig> = Lazy::new(AppConfig::from_env);

// db.rs - Database pool management
pub async fn init_db() -> Result<SqlitePool, sqlx::Error>;
pub async fn close_db();
```

### `src/services/receiver.rs`
**Most complex module** - Handles rate limiting and concurrency control

```rust
// Event-driven Token Bucket with Notify (no polling)
struct TokenBucket {
    tokens: AtomicU64,
    max_per_sec: u64,
    notify: Notify,
}

// Semaphore: Limits concurrent requests (max_per_sec * 2)
let semaphore = Arc::new(Semaphore::new(max_per_sec * 2));

// Bulk updates using CASE WHEN for message_id and error fields
bulk_update_all(&mut tx, &batch).await?;

// Vec::with_capacity() for pre-allocation
let mut status_binds: Vec<i32> = Vec::with_capacity(batch_len);

// Deferred clone: Tracking pixel added at send time (not creation time)
let mut content = (*request.content).clone();  // Clone only when sending
```

### `src/models/request.rs`
```rust
// Arc<String> for subject/content: Share single allocation across all emails
// 10,000 emails = 1 Arc::clone vs 10,000 String::clone
pub struct EmailRequest {
    pub subject: Arc<String>,
    pub content: Arc<String>,
    // ...
}

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
| GET | `/health` | ❌ | `health` |
| GET | `/ready` | ❌ | `ready` |
| POST | `/v1/messages` | ✅ | `create_message` |
| GET | `/v1/topics/{topic_id}` | ✅ | `get_topic` |
| DELETE | `/v1/topics/{topic_id}` | ✅ | `stop_topic` |
| GET | `/v1/events/open` | ❌ | `track_open` |
| GET | `/v1/events/counts/sent` | ✅ | `get_sent_count` |
| POST | `/v1/events/results` | ❌ | `handle_sns_event` |

---

## ⚙️ Environment Variables

### Server Configuration
| Variable | Required | Default | Description |
|------|:------:|--------|------|
| `SERVER_PORT` | ❌ | 8080 | Server port |
| `SERVER_URL` | ✅ | - | External access URL |
| `API_KEY` | ✅ | - | API authentication key |
| `RUST_LOG` | ❌ | info | Log level |

### AWS Configuration
| Variable | Required | Default | Description |
|------|:------:|--------|------|
| `AWS_REGION` | ❌ | ap-northeast-2 | AWS region |
| `AWS_SES_FROM_EMAIL` | ✅ | - | Sender email |

### Rate Limiting
| Variable | Required | Default | Description |
|------|:------:|--------|------|
| `MAX_SEND_PER_SECOND` | ❌ | 24 | Maximum sends per second |

### Database Configuration
| Variable | Required | Default | Description |
|------|:------:|--------|------|
| `DB_MAX_CONNECTIONS` | ❌ | 20 | Maximum connections |
| `DB_MIN_CONNECTIONS` | ❌ | 5 | Minimum connections |
| `DB_ACQUIRE_TIMEOUT_SECS` | ❌ | 30 | Connection acquire timeout (seconds) |
| `DB_IDLE_TIMEOUT_SECS` | ❌ | 300 | Idle connection timeout (seconds) |

### Channel Configuration
| Variable | Required | Default | Description |
|------|:------:|--------|------|
| `SEND_CHANNEL_BUFFER` | ❌ | 10,000 | Send channel buffer size |
| `POST_SEND_CHANNEL_BUFFER` | ❌ | 1,000 | Post-send channel buffer size |

### Monitoring
| Variable | Required | Default | Description |
|------|:------:|--------|------|
| `SENTRY_DSN` | ❌ | - | Sentry DSN |
| `SENTRY_TRACES_SAMPLE_RATE` | ❌ | 0.1 | Sentry trace sampling rate |

---

## 🔧 Development Environment

### Build and Run

```bash
# Development mode (migrations auto-applied)
cargo run

# Release mode
cargo run --release

# Test
cargo test

# Linting
cargo clippy
cargo fmt
```

### Database Migrations

Migrations are automatically applied on server startup via `config/db.rs`.

```bash
# Install SQLx CLI (optional, for manual migration management)
cargo install sqlx-cli --no-default-features --features native-tls,sqlite

# Create new migration
sqlx migrate add <description>

# Check migration status
sqlx migrate info
```

### Key Constants

| Constant | Value | Location |
|------|-----|------|
| `BATCH_SIZE` (scheduler) | 1,000 | scheduler.rs |
| `BATCH_INSERT_SIZE` | 150 | constants.rs |
| `BATCH_FLUSH_INTERVAL_MS` | 500 | receiver.rs |
| `TOKEN_REFILL_INTERVAL_MS` | 100 | receiver.rs |

---

## 📝 Rust Code Style Guide

> This project follows the [Rust Official Style Guide](https://doc.rust-lang.org/stable/style-guide/) and [Rust API Guidelines](https://rust-lang.github.io/api-guidelines/).

### rustfmt.toml Configuration

```toml
edition = "2021"
max_width = 100
tab_spaces = 4
newline_style = "Unix"
use_small_heuristics = "Default"

# Import configuration
imports_granularity = "Module"
group_imports = "StdExternalCrate"
reorder_imports = true
reorder_modules = true

# Function and struct formatting
fn_args_layout = "Tall"
struct_lit_single_line = true

# Comment formatting
wrap_comments = true
format_code_in_doc_comments = true
doc_comment_code_block_width = 80
```

### Lint Configuration

```toml
[lints.rust]
unsafe_code = "forbid"

[lints.clippy]
all = { level = "warn", priority = -1 }
pedantic = { level = "warn", priority = -1 }
nursery = { level = "warn", priority = -1 }
# Allowed patterns
module_name_repetitions = "allow"
must_use_candidate = "allow"
missing_errors_doc = "allow"
missing_panics_doc = "allow"
struct_field_names = "allow"
similar_names = "allow"
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

### Error Handling

```rust
// ✅ Good: Use centralized AppError
pub async fn get_topic(...) -> AppResult<impl IntoResponse> {
    if topic_id.is_empty() {
        return Err(AppError::BadRequest("topic_id is required".to_string()));
    }
    let counts = EmailRequest::get_counts(&state.db_pool, &topic_id).await?;
    Ok(Json(counts))
}

// ✅ Good: Use ? operator for error propagation
let count = EmailRequest::sent_count(&state.db_pool, hours).await?;

// ❌ Bad: Manual error handling in handlers
match result {
    Ok(data) => (StatusCode::OK, Json(data)).into_response(),
    Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, "Error").into_response(),
}
```

### Handler Function Patterns

```rust
// ✅ Good: Return AppResult<impl IntoResponse>
pub async fn get_topic(
    State(state): State<AppState>,
    Path(topic_id): Path<String>,
) -> AppResult<impl IntoResponse> {
    // Validation
    if topic_id.is_empty() {
        return Err(AppError::BadRequest("topic_id is required".to_string()));
    }
    // Business logic
    let counts = EmailRequest::get_counts(&state.db_pool, &topic_id).await?;
    Ok(Json(counts))
}
```

### Import Organization

```rust
// ✅ Good: Standard library → External crates → Internal modules order
use std::collections::HashMap;
use std::sync::Arc;

use axum::{extract::State, response::IntoResponse, Json};
use serde::{Deserialize, Serialize};

use crate::error::{AppError, AppResult};
use crate::models::request::EmailRequest;
use crate::state::AppState;
```

---

## 🧪 Test Code Style Guide

### Test Structure

Tests are now inline in each module using `#[cfg(test)] mod tests`:

```rust
// At the bottom of each module file
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_function_name_scenario() {
        // Arrange
        let input = "test_input";
        
        // Act
        let result = function_under_test(input);
        
        // Assert
        assert!(result.is_ok());
    }
}
```

### Async Test Pattern

```rust
#[cfg(test)]
mod tests {
    use super::*;

    async fn setup_db() -> SqlitePool {
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap();
        // Create tables...
        pool
    }

    #[tokio::test]
    async fn test_async_function() {
        let db = setup_db().await;
        let result = async_function(&db).await;
        assert!(result.is_ok());
    }
}
```

### Test Function Naming

```rust
// ✅ Good: test_ prefix + function_name + scenario
#[test]
fn test_app_error_bad_request_display() { }

#[tokio::test]
async fn test_bulk_update_all_unified() { }

// ❌ Bad: Unclear or overly long names
#[test]
fn test1() { }

#[test]
fn test_that_when_validating_it_should_fail_if_invalid() { }
```

---

## 🤖 AI Agent Guidelines

### DO's (Recommended Practices)

1. **Always run `cargo check` or `cargo build`** after making changes
2. **Run `cargo clippy`** to catch common mistakes
3. **Run `cargo fmt`** to ensure consistent formatting
4. **Run `cargo test`** after changes
5. **Use `AppError` and `AppResult`** for error handling
6. **Follow existing patterns** in the codebase
7. **Use `#[must_use]`** for functions returning important values
8. **Use `const fn`** for simple constructors
9. **Leverage `Arc`** for sharing data across async tasks

### DON'Ts (Avoid These)

1. **DON'T use `.unwrap()` or `.expect()`** in production code
2. **DON'T add `unsafe` code** - it's forbidden via lint
3. **DON'T ignore Clippy warnings**
4. **DON'T bypass authentication middleware** for protected routes
5. **DON'T use blocking operations** in async contexts
6. **DON'T hardcode configuration values** - use `APP_CONFIG`
7. **DON'T use `println!` for logging** - use `tracing` macros
8. **DON'T create separate test files** - use inline tests

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
4. **Clippy Pass**: No `cargo clippy` warnings (including nursery)
5. **Code Formatting**: Apply `cargo fmt`

### Pre-commit Checklist

```bash
cargo fmt --check
cargo clippy -- -D warnings
cargo test
```

---

## 📚 References

- [The Rust Programming Language Book](https://doc.rust-lang.org/book/)
- [Rust API Guidelines](https://rust-lang.github.io/api-guidelines/)
- [Rust Style Guide](https://doc.rust-lang.org/nightly/style-guide/)
- [Clippy Lints](https://rust-lang.github.io/rust-clippy/master/)
- [Axum Documentation](https://docs.rs/axum/latest/axum/)
- [SQLx Documentation](https://docs.rs/sqlx/latest/sqlx/)
- [AWS SDK for Rust](https://docs.rs/aws-sdk-sesv2/latest/aws_sdk_sesv2/)

---

*Last Updated: 2025-12-28*
