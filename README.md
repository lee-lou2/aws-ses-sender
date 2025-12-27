# 📧 AWS SES Email Sender

[한국어](README.ko.md) | [English](README.md)

A high-performance bulk email sending service built with **Rust** and **AWS SES**.

## ✨ Features

- 🚀 **Bulk Sending** — Up to 10,000 emails per request
- ⏰ **Scheduled Delivery** — Send emails at a specific time
- ⚡ **Rate Limiting** — Token Bucket + Semaphore for precise control
- 📊 **Event Tracking** — Bounce, Complaint, Delivery via AWS SNS
- 👀 **Open Tracking** — 1x1 transparent pixel detection
- ⏸️ **Cancellation** — Stop pending emails by topic

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

### How It Works

**Immediate Sending:**
1. API receives request → Batch INSERT to DB
2. Forward to sender channel → Rate-limited sending
3. Batch update results (100 per transaction)

**Scheduled Sending:**
1. API receives request with `scheduled_at`
2. Stored with `Created` status
3. Scheduler polls every 10s → Picks up due emails
4. Same flow as immediate sending

---

## ⚡ Performance

| Optimization | Description |
|--------------|-------------|
| **Token Bucket** | Precise per-second rate control with atomic CAS |
| **Semaphore** | Limits concurrent network requests (2× rate limit) |
| **WAL Mode** | SQLite concurrent reads during writes |
| **Batch INSERT** | Multi-row INSERT for 10× performance |
| **Batch Updates** | 100 results per transaction |
| **Connection Pool** | 5-20 DB connections with idle timeout |

---

## 🚀 Quick Start

### Prerequisites

- Rust 1.70+
- AWS account with SES configured
- (Optional) AWS SNS for event notifications

### 1. Clone & Setup

```bash
git clone https://github.com/your-repo/aws-ses-sender.git
cd aws-ses-sender

# Initialize database
./init_database.sh

# Create .env file
cp .env.example .env
```

### 2. Configure Environment

```env
# Required
SERVER_URL=https://your-domain.com
API_KEY=your-secure-api-key
AWS_SES_FROM_EMAIL=noreply@your-domain.com

# Optional
SERVER_PORT=8080
AWS_REGION=ap-northeast-2
MAX_SEND_PER_SECOND=24
SENTRY_DSN=your-sentry-dsn
RUST_LOG=info
```

### 3. Run

```bash
# Development
cargo run

# Production
cargo run --release

# Docker
docker build -t ses-sender .
docker run -p 8080:8080 --env-file .env ses-sender
```

---

## 📡 API Reference

### Authentication

All protected endpoints require the `X-API-KEY` header:

```http
X-API-KEY: your-api-key
```

### Endpoints

| Method | Endpoint | Auth | Description |
|--------|----------|------|-------------|
| POST | `/v1/messages` | ✅ | Send emails |
| GET | `/v1/topics/{id}` | ✅ | Get topic statistics |
| DELETE | `/v1/topics/{id}` | ✅ | Cancel pending emails |
| GET | `/v1/events/open` | ❌ | Track email opens |
| GET | `/v1/events/counts/sent` | ✅ | Get sent count |
| POST | `/v1/events/results` | ❌ | AWS SNS webhook |

### Send Emails

```http
POST /v1/messages
X-API-KEY: your-api-key
Content-Type: application/json
```

```json
{
  "messages": [
    {
      "topic_id": "newsletter_2024_01",
      "emails": ["user1@example.com", "user2@example.com"],
      "subject": "January Newsletter",
      "content": "<h1>Hello!</h1><p>Welcome to our newsletter.</p>"
    }
  ],
  "scheduled_at": "2024-01-15 09:00:00"
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

### Get Topic Statistics

```http
GET /v1/topics/newsletter_2024_01
X-API-KEY: your-api-key
```

**Response:**

```json
{
  "request_counts": {
    "Created": 50,
    "Sent": 945,
    "Failed": 5
  },
  "result_counts": {
    "Open": 423,
    "Bounce": 3,
    "Delivery": 942
  }
}
```

### Cancel Pending Emails

```http
DELETE /v1/topics/newsletter_2024_01
X-API-KEY: your-api-key
```

Only affects emails with `Created` status (not yet sent).

### Get Sent Count

```http
GET /v1/events/counts/sent?hours=24
X-API-KEY: your-api-key
```

**Response:**

```json
{
  "count": 1523
}
```

---

## 🔧 AWS Setup

### SES Configuration

1. **Verify Domain**
   - Go to AWS SES Console → Verified Identities
   - Add your domain and configure DKIM/SPF records

2. **Exit Sandbox** (Production)
   - Request production access via [AWS Support](https://docs.aws.amazon.com/ses/latest/dg/request-production-access.html)

3. **IAM Permissions**
   ```json
   {
     "Effect": "Allow",
     "Action": ["ses:SendEmail", "ses:SendRawEmail"],
     "Resource": "*"
   }
   ```

### SNS Configuration (Optional)

For event tracking (Bounce, Complaint, Delivery):

1. **Create SNS Topic**
   - AWS SNS Console → Create topic

2. **Configure SES Events**
   - SES Console → Configuration Sets → Event destinations
   - Add SNS destination for Bounce, Complaint, Delivery

3. **Subscribe Endpoint**
   - Add HTTP/HTTPS subscription: `https://your-domain.com/v1/events/results`
   - Confirm subscription (automatic via API)

![AWS Architecture](docs/aws_diagram.png)

---

## 📊 Monitoring

### Log Levels

```bash
RUST_LOG=debug cargo run  # Verbose output
RUST_LOG=info cargo run   # Normal operation
RUST_LOG=warn cargo run   # Warnings only
```

### Health Check

```bash
curl -H "X-API-KEY: $API_KEY" \
  http://localhost:8080/v1/events/counts/sent
```

### Sentry Integration

Set `SENTRY_DSN` environment variable to enable error tracking.

---

## 🧪 Testing

```bash
# Run all tests
cargo test

# Run with output
cargo test -- --nocapture

# Run specific test
cargo test test_save_batch

# Run specific module tests
cargo test request_tests
```

---

## 📁 Project Structure

```
src/
├── main.rs           # Entry point, initialization
├── app.rs            # Router configuration
├── config.rs         # Environment variables
├── state.rs          # Application state
├── handlers/         # HTTP handlers
│   ├── message_handlers.rs
│   ├── event_handlers.rs
│   └── topic_handlers.rs
├── services/         # Background services
│   ├── scheduler.rs  # Scheduled email pickup
│   ├── receiver.rs   # Rate-limited sending
│   └── sender.rs     # AWS SES client
├── models/           # Data models
│   ├── request.rs    # EmailRequest
│   └── result.rs     # EmailResult
├── middlewares/      # HTTP middlewares
│   └── auth_middlewares.rs
└── tests/            # Test modules
```

---

## 🛠 Development

### Code Quality

```bash
# Format code
cargo fmt

# Run linter
cargo clippy

# Build release
cargo build --release
```

### Dependencies

| Crate | Purpose |
|-------|---------|
| `axum` | Web framework |
| `tokio` | Async runtime |
| `sqlx` | Database (SQLite) |
| `aws-sdk-sesv2` | AWS SES client |
| `serde` | Serialization |
| `thiserror` | Error handling |
| `tracing` | Logging |
| `sentry` | Error tracking |

---

## 📄 License

MIT License

---

## 📚 References

- [AWS SES Documentation](https://docs.aws.amazon.com/ses/latest/dg/Welcome.html)
- [AWS SNS Documentation](https://docs.aws.amazon.com/sns/latest/dg/welcome.html)
- [Axum Documentation](https://docs.rs/axum)
- [SQLx Documentation](https://docs.rs/sqlx)
