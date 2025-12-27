# 📧 AWS SES Email Sender

[한국어](README.ko.md) | [English](README.md)

A high-performance email sending and monitoring server utilizing AWS SES and SNS.
Built with Rust and Tokio for exceptional throughput and reliability.

## 🏗 System Architecture

### Tech Stack
- 🦀 **Backend**: Rust + Axum
- 📨 **Email Service**: AWS SES
- 🔔 **Notification**: AWS SNS
- 🔄 **Async Runtime**: Tokio
- 💾 **Database**: SQLite
- 🔒 **Auth**: X-API-KEY Header
- 📊 **Monitoring**: Sentry + tracing

### How It Works

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

#### Immediate Sending
1. Receive API request (`/v1/messages`)
2. **Batch INSERT** to DB → Forward to sender channel
3. Rate-limited sending via Token Bucket + Semaphore
4. Batch update results (100 per transaction)

#### Scheduled Sending
1. Receive API request (with `scheduled_at`)
2. Store with `Created` status
3. Scheduler polls every 10s, atomically claims due emails (UPDATE...RETURNING)
4. Same sending flow as immediate

## ⚡ Performance Optimizations

### Rate Limiting (Token Bucket + Semaphore)
- **Token Bucket**: Precise per-second rate control with atomic CAS
- **Semaphore**: Limits concurrent network requests (2x rate limit)
- **Smooth refill**: 10% tokens every 100ms for even distribution

### Database (SQLite + WAL)
- **WAL mode**: Concurrent reads during writes
- **mmap**: 256MB memory-mapped I/O
- **Cache**: 64MB in-memory cache + temp_store in memory
- **Auto vacuum**: Incremental vacuum for storage optimization
- **Batch INSERT**: Multi-row INSERT provides **10x+** performance
- **Batch updates**: Bulk status updates per transaction
- **Composite Indexes**: Optimized for scheduler, count, and stop queries
- **Content deduplication**: Subject/content stored separately to prevent duplication

### Connection Pooling
- **SES Client**: Single cached instance (OnceCell)
- **DB Pool**: 5-20 connections with idle timeout
- **Channels**: 10,000 send buffer, 1,000 post-send buffer

## ✨ Key Features

- 🚀 Bulk email sending and scheduling
- 📊 Real-time delivery monitoring
- 👀 Email open tracking (1x1 pixel)
- ⏸ Cancel pending email sends
- 📈 Per-topic statistics

![img_2.png](docs/process_diagram_en.png)

## 🔧 Setup Guide

### AWS SES Configuration

#### 1️⃣ Sandbox Mode Removal (Production)
- Request sandbox removal through [AWS Support Center](https://docs.aws.amazon.com/ses/latest/dg/request-production-access.html)

#### 2️⃣ Domain Authentication
- Register domain in AWS SES console
- Add DKIM and SPF records to DNS

#### 3️⃣ Email Address Verification (Sandbox Mode)
- Register sender email in AWS SES console

### AWS SNS Configuration (Optional)

#### 1️⃣ Create SNS Topic
- Create new topic in AWS SNS console

#### 2️⃣ SES Event Configuration
- Add SNS event destination (Bounce, Complaint, Delivery)

#### 3️⃣ SNS Subscription Setup
- Add subscription (HTTP/HTTPS endpoint: `/v1/events/results`)

![img_1.png](docs/aws_diagram.png)

## ⚙️ Environment Variables

```env
# AWS Configuration
AWS_REGION=ap-northeast-2
AWS_ACCESS_KEY_ID=your_access_key
AWS_SECRET_ACCESS_KEY=your_secret_key
AWS_SES_FROM_EMAIL=your_verified_email

# Server Configuration
SERVER_URL=http://localhost:3000
SERVER_PORT=3000
API_KEY=your_api_key
MAX_SEND_PER_SECOND=24

# Optional
SENTRY_DSN=your_sentry_dsn
RUST_LOG=info
```

## 🚀 Quick Start

```bash
# Initialize database
./init_database.sh

# Run server
cargo run --release

# Or with Docker
docker build -t ses-sender .
docker run -p 3000:3000 --env-file .env ses-sender
```

## 📡 API Guide

### Send Email

```http
POST /v1/messages
X-API-KEY: {your_api_key}
```

```json
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
```

**Response:**
```json
{
  "total": 2,
  "success": 2,
  "errors": 0,
  "duration_ms": 45,
  "scheduled": true
}
```

### Event Tracking

| Endpoint | Method | Description |
|----------|--------|-------------|
| `/v1/events/open?request_id={id}` | GET | Track email opens (returns 1x1 PNG) |
| `/v1/events/counts/sent?hours=24` | GET | Get sent count (last N hours) |
| `/v1/events/results` | POST | Receive AWS SNS events |

### Topic Management

| Endpoint | Method | Description |
|----------|--------|-------------|
| `/v1/topics/{topic_id}` | GET | Get topic statistics |
| `/v1/topics/{topic_id}` | DELETE | Cancel pending emails |

## 🧪 Testing

```bash
# Run all tests
cargo test

# Run with output
cargo test -- --nocapture

# Run specific test
cargo test test_save_batch
```

## 📊 Monitoring

### Log Levels
```bash
RUST_LOG=debug cargo run  # Detailed logs
RUST_LOG=info cargo run   # Normal operation
RUST_LOG=warn cargo run   # Warnings only
```

### Health Check
```bash
curl http://localhost:3000/v1/events/counts/sent \
  -H "X-API-KEY: $API_KEY"
```

## 📁 Project Structure

```
src/
├── main.rs                 # Entry point, initialization
├── app.rs                  # Router configuration
├── config.rs               # Environment variables
├── state.rs                # Application state
├── handlers/               # HTTP request handlers
│   ├── message_handlers.rs # Email sending API
│   ├── event_handlers.rs   # SNS events, open tracking
│   └── topic_handlers.rs   # Topic management
├── services/               # Background services
│   ├── scheduler.rs        # Scheduled email pickup
│   ├── receiver.rs         # Rate-limited sending
│   └── sender.rs           # AWS SES API calls
├── models/                 # Data models
│   ├── content.rs          # EmailContent (subject, content storage)
│   ├── request.rs          # EmailRequest, EmailMessageStatus
│   └── result.rs           # EmailResult
├── middlewares/            # HTTP middlewares
│   └── auth_middlewares.rs # API key authentication
└── tests/                  # Unit & integration tests
    ├── helpers (mod.rs)    # Shared test utilities
    ├── auth_tests.rs
    ├── event_tests.rs
    ├── handler_tests.rs
    ├── request_tests.rs
    ├── scheduler_tests.rs
    ├── status_tests.rs
    └── topic_tests.rs
```

## 🛠 Development Guide

### Code Style

This project follows the official Rust style guide:

```bash
# Format code
cargo fmt

# Run linter
cargo clippy

# Run with all checks
cargo clippy -- -W clippy::all -W clippy::pedantic
```

**Lint Configuration (Cargo.toml):**
```toml
[lints.rust]
unsafe_code = "forbid"

[lints.clippy]
all = "warn"
pedantic = "warn"
nursery = "warn"
```

### Dependencies

| Crate | Purpose |
|-------|---------|
| `axum` | Web framework |
| `tokio` | Async runtime |
| `sqlx` | Database (SQLite) |
| `aws-sdk-sesv2` | AWS SES API |
| `serde` / `serde_json` | Serialization |
| `thiserror` | Error handling |
| `tracing` | Logging |
| `sentry` | Error tracking |

### Building

```bash
# Development
cargo build

# Release (optimized)
cargo build --release

# Check without building
cargo check
```

## 📚 References

- [AWS SES Developer Guide](https://docs.aws.amazon.com/ses/latest/dg/Welcome.html)
- [AWS SNS Developer Guide](https://docs.aws.amazon.com/sns/latest/dg/welcome.html)
- [Axum Documentation](https://docs.rs/axum)
- [SQLx Documentation](https://docs.rs/sqlx)
- [Rust API Guidelines](https://rust-lang.github.io/api-guidelines/)

## 📄 License

MIT License
