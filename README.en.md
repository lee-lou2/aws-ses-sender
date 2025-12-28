# AWS SES Email Sender

[한국어](README.md) | [English](README.en.md)

A high-performance bulk email sending and monitoring server powered by AWS SES and SNS.
Built with Rust and Tokio for exceptional throughput and reliability.

## Key Features

| Feature | Description |
|---------|-------------|
| Bulk Sending | Up to 10,000 emails per request |
| Scheduled Sending | Specify with `scheduled_at` parameter |
| Real-time Monitoring | Receive delivery results via AWS SNS |
| Open Tracking | Track opens with 1x1 transparent pixel |
| Cancel Sending | Cancel pending emails by topic |
| Topic Statistics | View delivery status by topic |

## Tech Stack

| Component | Technology |
|-----------|------------|
| Backend | Rust + Axum |
| Email Service | AWS SES v2 |
| Notification | AWS SNS |
| Async Runtime | Tokio |
| Database | SQLite (WAL mode) |
| Auth | X-API-KEY Header |
| Monitoring | Sentry + tracing |

---

## System Architecture

### Overall System Flow

```mermaid
flowchart TB
    subgraph Client["Client"]
        API[API Request]
    end

    subgraph Server["Email Sending Server"]
        Handler[Message Handler]
        Scheduler[Scheduler]
        Sender[Sender Service]
        Receiver[Receiver Service]
    end

    subgraph Database["Database"]
        SQLite[(SQLite WAL)]
    end

    subgraph AWS["AWS Cloud"]
        SES[AWS SES v2]
        SNS[AWS SNS]
    end

    API -->|POST /v1/messages| Handler
    Handler -->|Batch INSERT| SQLite
    Handler -->|Immediate Send| Sender

    Scheduler -->|Poll every 10s| SQLite
    Scheduler -->|Pick up scheduled| Sender

    Sender -->|Rate Limited| SES
    SES -->|Delivery Events| SNS
    SNS -->|Webhook| Receiver
    Receiver -->|Status Update| SQLite
```

### Email Sending Process

```mermaid
flowchart LR
    subgraph Input["Request Reception"]
        A[API Request] --> B{scheduled_at?}
    end

    subgraph Immediate["Immediate Sending"]
        B -->|No| C[Save to DB]
        C --> D[Send to Channel]
        D --> E[Rate Limiter]
    end

    subgraph Scheduled["Scheduled Sending"]
        B -->|Yes| F[Save as Created]
        F --> G[Scheduler Poll]
        G -->|Every 10s| H[UPDATE...RETURNING]
        H --> E
    end

    subgraph Sending["Send Processing"]
        E --> I[Acquire Token]
        I --> J[Call AWS SES]
        J --> K[Update Result]
    end

    subgraph Result["Result Reception"]
        L[AWS SNS] -->|Webhook| M[Receiver]
        M --> N[Batch UPDATE]
    end

    J -.-> L
```

### Immediate Sending Sequence

When a client requests without `scheduled_at`, emails are sent immediately.

```mermaid
sequenceDiagram
    autonumber
    participant C as Client
    participant H as Handler
    participant DB as SQLite
    participant S as Sender
    participant SES as AWS SES

    C->>H: POST /v1/messages
    H->>DB: Save Content (dedup)
    H->>DB: Batch INSERT (150 rows)
    H->>S: Send via Channel
    H-->>C: Return Response

    loop Rate Limited (Token Bucket)
        S->>S: Wait for Token
        S->>SES: Send Email
        SES-->>S: Return Message ID
        S->>DB: Batch UPDATE (100 rows)
    end
```

### Scheduled Sending Sequence

Requests with `scheduled_at` wait until the specified time before sending.

```mermaid
sequenceDiagram
    autonumber
    participant C as Client
    participant H as Handler
    participant DB as SQLite
    participant Sch as Scheduler
    participant S as Sender
    participant SES as AWS SES

    C->>H: POST /v1/messages<br/>(with scheduled_at)
    H->>DB: Save Content
    H->>DB: Save as Created status
    H-->>C: Return Response (scheduled: true)

    loop Poll every 10 seconds
        Sch->>DB: UPDATE...RETURNING<br/>(atomic pickup)
        DB-->>Sch: Emails to send
        Sch->>S: Send via Channel
    end

    loop Rate Limited
        S->>SES: Send Email
        SES-->>S: Return Message ID
        S->>DB: Update Status
    end
```

### SNS Event Processing

Receive delivery results from AWS SES via SNS.

```mermaid
flowchart LR
    subgraph AWS["AWS Cloud"]
        SES[AWS SES]
        SNS[AWS SNS]
    end

    subgraph Events["Event Types"]
        D[Delivery<br/>Success]
        B[Bounce<br/>Rejected]
        C[Complaint<br/>Spam Report]
    end

    subgraph Server["Email Server"]
        R[Receiver Service]
        DB[(SQLite)]
    end

    SES -->|Delivery Events| SNS
    SNS --> D & B & C
    D & B & C -->|POST /v1/events/results| R
    R -->|Batch UPDATE| DB
```

### Rate Limiting Architecture

Event-driven approach combining Token Bucket and Semaphore.

```mermaid
flowchart TB
    subgraph TokenBucket["Token Bucket"]
        T[Token Pool]
        R[Refill 10%<br/>every 100ms]
    end

    subgraph Semaphore["Semaphore"]
        S[Concurrent Limit]
        N["Rate Limit x 2"]
    end

    subgraph Sender["Sending Process"]
        A[1. Acquire Token]
        B[2. Acquire Semaphore]
        C[3. Call SES API]
        D[4. Release Resources]
    end

    R -.->|Notify| T
    T --> A
    A --> B
    S --> B
    B --> C
    C --> D
    D -.->|Return Token| T
    D -.->|Return Permit| S
```

### Database Schema

```mermaid
erDiagram
    EMAIL_CONTENT {
        string id PK
        string subject
        string content
        datetime created_at
    }

    EMAIL_REQUEST {
        string id PK
        string topic_id
        string email
        string content_id FK
        string status
        string message_id
        datetime scheduled_at
        datetime created_at
    }

    EMAIL_RESULT {
        string id PK
        string request_id FK
        string result_type
        string bounce_type
        datetime created_at
    }

    EMAIL_CONTENT ||--o{ EMAIL_REQUEST : "has"
    EMAIL_REQUEST ||--o| EMAIL_RESULT : "has"
```

---

## Performance Optimizations

### Rate Limiting

| Component | Method | Feature |
|-----------|--------|---------|
| Token Bucket | `Notify` based | Event-driven, no polling |
| Semaphore | Concurrent limit | 2x Rate Limit |
| Refill | 10% every 100ms | Even distribution |

### Database

| Optimization | Effect |
|--------------|--------|
| WAL Mode | Concurrent reads during writes |
| mmap 256MB | Memory-mapped I/O |
| Cache 64MB | In-memory cache |
| Batch INSERT | 10x+ performance improvement |
| CASE WHEN UPDATE | Bulk updates |
| UPDATE...RETURNING | Atomic scheduler pickup |
| Composite Indexes | Query optimization |

### Memory Optimization

| Technique | Effect |
|-----------|--------|
| `Arc<String>` | Share subject/content (1 allocation for 10,000 emails) |
| `Vec::with_capacity()` | Prevent reallocations |
| Lazy Copy | Add tracking pixel at send time |

### Connection Management

| Resource | Setting |
|----------|---------|
| SES Client | OnceCell singleton |
| DB Pool | 5-20 connections |
| Send Channel | 10,000 buffer |
| Post-process Channel | 1,000 buffer |

---

## Setup Guide

### AWS SES Configuration

1. **Sandbox Removal** (Production)
   - [Request via AWS Support Center](https://docs.aws.amazon.com/ses/latest/dg/request-production-access.html)

2. **Domain Authentication**
   - Register domain in AWS SES console
   - Add DKIM and SPF records to DNS

3. **Email Verification** (Sandbox)
   - Register sender email address

### AWS SNS Configuration (Optional)

1. Create SNS topic
2. Add SES event destination (Bounce, Complaint, Delivery)
3. Set up HTTP subscription (`/v1/events/results`)

---

## Environment Variables

| Variable | Required | Default | Description |
|----------|:--------:|---------|-------------|
| `SERVER_PORT` | | 8080 | Server port |
| `SERVER_URL` | O | | External access URL |
| `API_KEY` | O | | API authentication key |
| `AWS_REGION` | | ap-northeast-2 | AWS region |
| `AWS_ACCESS_KEY_ID` | O | | AWS access key |
| `AWS_SECRET_ACCESS_KEY` | O | | AWS secret key |
| `AWS_SES_FROM_EMAIL` | O | | Verified sender email |
| `MAX_SEND_PER_SECOND` | | 24 | Maximum sends per second |
| `SENTRY_DSN` | | | Sentry DSN |
| `RUST_LOG` | | info | Log level |

---

## Quick Start

```bash
# Run server (migrations auto-applied)
cargo run --release

# Docker
docker build -t ses-sender .
docker run -p 3000:3000 --env-file .env ses-sender
```

> Database migrations are automatically applied on server startup (`migrations/` folder)

---

## API Reference

### Send Email

```http
POST /v1/messages
X-API-KEY: {api_key}
Content-Type: application/json
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

### Event API

| Endpoint | Method | Description |
|----------|:------:|-------------|
| `/v1/events/open?request_id={id}` | GET | Open tracking (1x1 PNG) |
| `/v1/events/counts/sent?hours=24` | GET | Get sent count |
| `/v1/events/results` | POST | Receive SNS events |

### Topic API

| Endpoint | Method | Description |
|----------|:------:|-------------|
| `/v1/topics/{topic_id}` | GET | Get statistics |
| `/v1/topics/{topic_id}` | DELETE | Cancel pending emails |

### Health Check

| Endpoint | Description | Auth |
|----------|-------------|:----:|
| `/health` | Basic health check | |
| `/ready` | DB connection check | |

---

## Project Structure

```
src/
├── main.rs                 # Entry point, Graceful Shutdown
├── app.rs                  # Router configuration
├── config.rs               # Environment variables
├── constants.rs            # Constants (BATCH_INSERT_SIZE)
├── state.rs                # Application state
├── handlers/
│   ├── message_handlers.rs # Email sending API
│   ├── event_handlers.rs   # SNS events, open tracking
│   ├── health_handlers.rs  # Health checks
│   └── topic_handlers.rs   # Topic management
├── services/
│   ├── scheduler.rs        # Scheduled email pickup
│   ├── receiver.rs         # Rate-limited sending, batch updates
│   └── sender.rs           # AWS SES API calls
├── models/
│   ├── content.rs          # EmailContent
│   ├── request.rs          # EmailRequest (Arc<String>)
│   └── result.rs           # EmailResult
├── middlewares/
│   └── auth_middlewares.rs # API Key authentication
└── tests/                  # Tests
```

---

## Development

### Code Style

```bash
cargo fmt                   # Formatting
cargo clippy               # Linter
```

### Build

```bash
cargo build                # Development
cargo build --release      # Release
cargo check                # Check only
```

### Testing

```bash
cargo test                      # All tests
cargo test -- --nocapture      # With output
cargo test test_save_batch     # Specific test
```

### Monitoring

```bash
RUST_LOG=debug cargo run  # Detailed logs
RUST_LOG=info cargo run   # Normal operation
RUST_LOG=warn cargo run   # Warnings only
```

### Key Dependencies

| Crate | Purpose |
|-------|---------|
| axum | Web framework |
| tokio | Async runtime |
| sqlx | SQLite |
| aws-sdk-sesv2 | AWS SES |
| serde | Serialization |
| tracing | Logging |
| sentry | Error tracking |

---

## References

- [AWS SES Developer Guide](https://docs.aws.amazon.com/ses/latest/dg/Welcome.html)
- [AWS SNS Developer Guide](https://docs.aws.amazon.com/sns/latest/dg/welcome.html)
- [Axum Documentation](https://docs.rs/axum)
- [SQLx Documentation](https://docs.rs/sqlx)

## License

MIT License
