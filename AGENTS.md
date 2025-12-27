# AGENTS.md

> AI 코딩 에이전트를 위한 프로젝트 가이드

---

## 📋 프로젝트 개요

**aws-ses-sender**는 AWS SES를 통한 고성능 대량 이메일 발송 서비스입니다.

### 핵심 기능
- 🚀 **대량 이메일 발송**: 요청당 최대 10,000개 이메일 처리
- ⏰ **예약 발송**: `scheduled_at` 필드로 미래 시점 발송 예약
- 📊 **이벤트 추적**: AWS SNS를 통한 Bounce/Complaint/Delivery 이벤트 수신
- 👀 **오픈 트래킹**: 1x1 투명 픽셀을 통한 이메일 열람 추적
- ⚡ **Rate Limiting**: Token Bucket + Semaphore 기반 초당 발송량 제어

### 기술 스택
| 영역 | 기술 |
|------|------|
| 언어 | Rust 2021 Edition |
| 웹 프레임워크 | Axum 0.8 |
| 비동기 런타임 | Tokio |
| 데이터베이스 | SQLite (WAL 모드) |
| 이메일 서비스 | AWS SES v2 |
| 인증 | X-API-KEY 헤더 |
| 에러 트래킹 | Sentry |

---

## 🏗 아키텍처

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

### 데이터 흐름
1. **즉시 발송**: API → 배치 INSERT → 발송 채널 → Rate-limited 발송 → 결과 배치 업데이트
2. **예약 발송**: API → 배치 INSERT (Created) → 스케줄러 폴링 → 발송 채널 → 발송 → 결과 업데이트

---

## 📁 프로젝트 구조

```
src/
├── main.rs                 # 진입점, 초기화, 백그라운드 태스크 스폰
├── app.rs                  # Axum 라우터 설정
├── config.rs               # 환경변수 로드 (싱글톤)
├── state.rs                # AppState 정의 (DB 풀, 채널)
├── handlers/               # HTTP 요청 핸들러
│   ├── mod.rs
│   ├── message_handlers.rs # POST /v1/messages
│   ├── event_handlers.rs   # GET/POST /v1/events/*
│   └── topic_handlers.rs   # GET/DELETE /v1/topics/{id}
├── services/               # 백그라운드 서비스
│   ├── mod.rs
│   ├── scheduler.rs        # 예약 이메일 폴링 (10초 간격)
│   ├── receiver.rs         # Rate-limited 발송 + 배치 DB 업데이트
│   └── sender.rs           # AWS SES API 호출 (싱글톤 클라이언트)
├── models/                 # 데이터 모델
│   ├── mod.rs
│   ├── request.rs          # EmailRequest, EmailMessageStatus
│   └── result.rs           # EmailResult
├── middlewares/            # HTTP 미들웨어
│   ├── mod.rs
│   └── auth_middlewares.rs # API Key 인증
└── tests/                  # 테스트
    ├── mod.rs              # 공유 헬퍼 함수
    ├── auth_tests.rs
    ├── event_tests.rs
    ├── handler_tests.rs
    ├── request_tests.rs
    └── status_tests.rs
```

---

## 🔑 핵심 모듈

### `src/main.rs`
- 애플리케이션 진입점
- 로거, Sentry, DB 초기화
- 3개의 백그라운드 태스크 스폰

### `src/services/receiver.rs`
**가장 복잡한 모듈** - Rate limiting과 동시성 제어 담당

```rust
// Token Bucket: 초당 발송량 제어
let tokens = Arc::new(AtomicU64::new(max_per_sec));

// Semaphore: 동시 요청 수 제한 (max_per_sec * 2)
let semaphore = Arc::new(Semaphore::new(max_per_sec * 2));
```

### `src/models/request.rs`
```rust
pub enum EmailMessageStatus {
    Created = 0,    // 생성됨 (예약 발송 대기)
    Processed = 1,  // 처리됨 (발송 큐에 등록)
    Sent = 2,       // 발송 완료
    Failed = 3,     // 발송 실패
    Stopped = 4,    // 발송 중단됨
}
```

---

## 🗄 데이터베이스 스키마

### `email_requests` 테이블
| 컬럼 | 타입 | 설명 |
|------|------|------|
| id | INTEGER PK | 자동 증가 ID |
| topic_id | VARCHAR(255) | 그룹 발송 식별자 |
| message_id | VARCHAR(255) | AWS SES 메시지 ID |
| email | VARCHAR(255) | 수신자 이메일 |
| subject | VARCHAR(255) | 제목 |
| content | TEXT | HTML 본문 |
| scheduled_at | DATETIME | 예약 발송 시간 |
| status | TINYINT | EmailMessageStatus 값 |
| error | VARCHAR(255) | 에러 메시지 |
| created_at | DATETIME | 생성 시간 |
| updated_at | DATETIME | 수정 시간 |

### `email_results` 테이블
| 컬럼 | 타입 | 설명 |
|------|------|------|
| id | INTEGER PK | 자동 증가 ID |
| request_id | INTEGER FK | email_requests.id 참조 |
| status | VARCHAR(50) | 이벤트 유형 |
| raw | TEXT | 원본 SNS JSON |
| created_at | DATETIME | 생성 시간 |

---

## 🌐 API 엔드포인트

| 메서드 | 경로 | 인증 | 핸들러 함수 |
|--------|------|------|-------------|
| POST | `/v1/messages` | ✅ | `create_message` |
| GET | `/v1/topics/{topic_id}` | ✅ | `get_topic` |
| DELETE | `/v1/topics/{topic_id}` | ✅ | `stop_topic` |
| GET | `/v1/events/open` | ❌ | `track_open` |
| GET | `/v1/events/counts/sent` | ✅ | `get_sent_count` |
| POST | `/v1/events/results` | ❌ | `handle_sns_event` |

---

## ⚙️ 환경변수

| 변수 | 필수 | 기본값 | 설명 |
|------|------|--------|------|
| `SERVER_PORT` | ❌ | 8080 | 서버 포트 |
| `SERVER_URL` | ✅ | - | 외부 접근 URL |
| `API_KEY` | ✅ | - | API 인증 키 |
| `AWS_REGION` | ❌ | ap-northeast-2 | AWS 리전 |
| `AWS_SES_FROM_EMAIL` | ✅ | - | 발신자 이메일 |
| `MAX_SEND_PER_SECOND` | ❌ | 24 | 초당 최대 발송량 |
| `SENTRY_DSN` | ❌ | - | Sentry DSN |
| `RUST_LOG` | ❌ | info | 로그 레벨 |

---

## 🔧 개발 환경

### 빌드 및 실행

```bash
# 개발 모드
cargo run

# 릴리즈 모드
cargo run --release

# 테스트
cargo test

# 린팅
cargo clippy
cargo fmt
```

### 주요 상수

| 상수 | 값 | 위치 |
|------|-----|------|
| `DB_MAX_CONNECTIONS` | 20 | main.rs |
| `SEND_CHANNEL_BUFFER` | 10,000 | main.rs |
| `BATCH_SIZE` (scheduler) | 1,000 | scheduler.rs |
| `BATCH_INSERT_SIZE` | 100 | request.rs |
| `BATCH_FLUSH_INTERVAL_MS` | 500 | receiver.rs |

---

## 📝 Rust 코드 스타일 가이드

> 이 프로젝트는 [Rust 공식 스타일 가이드](https://doc.rust-lang.org/stable/style-guide/)와 [Rust API Guidelines](https://rust-lang.github.io/api-guidelines/)를 따릅니다.

### Lint 설정

```toml
[lints.rust]
unsafe_code = "forbid"

[lints.clippy]
all = "warn"
pedantic = "warn"
nursery = "warn"
```

### 네이밍 컨벤션

| 항목 | 스타일 | 예시 |
|------|--------|------|
| 크레이트/모듈 | `snake_case` | `email_sender`, `auth_middlewares` |
| 타입/트레이트 | `PascalCase` | `EmailRequest`, `SendEmailError` |
| 함수/메서드 | `snake_case` | `send_email`, `get_topic` |
| 상수 | `SCREAMING_SNAKE_CASE` | `MAX_EMAILS_PER_REQUEST`, `BATCH_SIZE` |
| 변수/파라미터 | `snake_case` | `db_pool`, `topic_id` |
| 라이프타임 | 짧은 소문자 | `'a`, `'de` |
| 타입 파라미터 | 단일 대문자 또는 `PascalCase` | `T`, `E`, `Item` |

### 모듈 문서 주석

```rust
// ✅ Good: 한 줄로 간결하게
//! Email request model and database operations

// ❌ Bad: 불필요하게 길고 장황한 설명
//! 이 모듈은 이메일 요청 모델과 데이터베이스 작업을 담당합니다.
//! 
//! ## 주요 기능
//! - 이메일 요청 저장
//! - 이메일 요청 조회
//! ...
```

### 함수 문서 주석

```rust
// ✅ Good: 필요한 경우에만 간결하게
/// Saves multiple requests in a single transaction using multi-row INSERT.
///
/// This provides ~10x performance improvement over individual inserts.
pub async fn save_batch(requests: Vec<Self>, db_pool: &SqlitePool) -> Result<Vec<Self>, sqlx::Error>

// ✅ Good: 단순한 함수는 문서 생략 가능
pub async fn update(&self, db_pool: &SqlitePool) -> Result<(), sqlx::Error>

// ❌ Bad: 코드에서 명백한 내용을 반복
/// This function updates the email request in the database
/// It takes a database pool and updates the request
pub async fn update(&self, db_pool: &SqlitePool) -> Result<(), sqlx::Error>
```

### 구분선 주석 금지

```rust
// ❌ Bad: 구분선 주석 사용
// =============================================================================
// Configuration
// =============================================================================
const BATCH_SIZE: usize = 100;

// ✅ Good: 관련 상수를 그룹으로 배치 (공백으로 구분)
// Token bucket configuration
const TOKEN_REFILL_INTERVAL_MS: u64 = 100;
const TOKEN_WAIT_INTERVAL_MS: u64 = 5;

// Batch update configuration
const BATCH_SIZE: usize = 100;
const BATCH_FLUSH_INTERVAL_MS: u64 = 500;
```

### Import 정리

```rust
// ✅ Good: 표준 라이브러리 → 외부 크레이트 → 내부 모듈 순서
use std::collections::HashMap;
use std::sync::Arc;

use axum::{extract::State, http::StatusCode};
use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;

use crate::models::request::EmailRequest;
use crate::state::AppState;
```

### 에러 처리

```rust
// ✅ Good: thiserror 사용
#[derive(Debug, Error)]
pub enum SendEmailError {
    #[error("Failed to build email: {0}")]
    Build(String),

    #[error("SES SDK error: {0}")]
    Sdk(String),
}

// ✅ Good: let-else 패턴 활용
let Some(ses_msg_id) = ses_msg_id else {
    error!("SES message_id not found");
    return (StatusCode::BAD_REQUEST, "Not found").into_response();
};

// ✅ Good: ? 연산자 활용
let row: (i64,) = sqlx::query_as("SELECT id FROM ...")
    .bind(message_id)
    .fetch_one(db_pool)
    .await?;
```

### 조건부 컴파일

```rust
// ✅ Good: 테스트 전용 함수
#[cfg(test)]
pub async fn save(self, db_pool: &SqlitePool) -> Result<Self, sqlx::Error> {
    // 테스트에서만 사용되는 개별 저장 로직
}
```

### 타입 변환

```rust
// ✅ Good: 명시적 캐스팅과 allow 속성
#[allow(clippy::cast_possible_truncation)]
let id = row.0 as i32;

// ✅ Good: 안전한 변환
let max_per_sec = u64::try_from(envs.max_send_per_second.max(1)).unwrap_or(1);
```

### 핸들러 함수 네이밍

```rust
// ✅ Good: 동사로 시작하는 간결한 이름
pub async fn create_message(...) -> impl IntoResponse
pub async fn get_topic(...) -> impl IntoResponse
pub async fn stop_topic(...) -> impl IntoResponse
pub async fn track_open(...) -> impl IntoResponse
pub async fn handle_sns_event(...) -> impl IntoResponse

// ❌ Bad: 불필요한 접미사
pub async fn create_message_handler(...) -> impl IntoResponse
pub async fn retrieve_topic_handler(...) -> impl IntoResponse
```

### 미들웨어 네이밍

```rust
// ✅ Good: 간결한 이름
pub async fn api_key_auth(req: Request<Body>, next: Next) -> impl IntoResponse

// ❌ Bad: 불필요한 접미사
pub async fn api_key_auth_middleware(req: Request<Body>, next: Next) -> impl IntoResponse
```

### 상수 정의

```rust
// ✅ Good: 관련 상수는 모듈 상단에 그룹으로
const MAX_BODY_SIZE: usize = 1024 * 1024; // 1MB

/// 1x1 transparent PNG for email open tracking
const TRACKING_PIXEL: &[u8] = &[
    0x89, 0x50, 0x4E, 0x47, ...
];

// ✅ Good: 주석은 필요할 때만
const DB_MAX_CONNECTIONS: u32 = 20;
const DB_MIN_CONNECTIONS: u32 = 5;
```

---

## 🧪 테스트 코드 스타일 가이드

### 테스트 파일 구조

```rust
#[cfg(test)]
mod tests {
    use crate::models::request::EmailRequest;
    use crate::tests::helpers::{get_api_key, setup_db};
    // ... other imports

    // 테스트 함수들
}
```

### 공유 헬퍼 함수

`tests/mod.rs`에 공유 헬퍼를 정의합니다:

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

        // 테이블 생성
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

### 테스트 함수 네이밍

```rust
// ✅ Good: test_ 접두사 + 테스트 대상 + 예상 결과
#[tokio::test]
async fn test_save_returns_id() { }

#[tokio::test]
async fn test_sent_count_empty() { }

#[tokio::test]
async fn test_stop_topic_updates_created_only() { }

// ❌ Bad: 불명확하거나 너무 긴 이름
#[tokio::test]
async fn test1() { }

#[tokio::test]
async fn test_that_when_we_save_an_email_request_it_should_return_the_id() { }
```

### 테스트 헬퍼 함수

```rust
// ✅ Good: 반복되는 테스트 데이터 생성 함수
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

### API 테스트 패턴

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

### 데이터베이스 테스트 패턴

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

### Assertion 스타일

```rust
// ✅ Good: 명확한 assertion
assert_eq!(response.status(), StatusCode::OK);
assert_eq!(saved.id, Some(1));
assert!(counts.is_empty());

// ✅ Good: 실패 시 유용한 메시지
assert_eq!(counts.get("Created"), Some(&2), "Created count mismatch");

// ❌ Bad: 불명확한 assertion
assert!(response.status() == StatusCode::OK);
```

### 테스트 분류

```
tests/
├── mod.rs              # 공유 헬퍼
├── auth_tests.rs       # 인증 관련 테스트
├── event_tests.rs      # 이벤트 핸들러 테스트
├── handler_tests.rs    # 메시지/토픽 핸들러 테스트
├── request_tests.rs    # EmailRequest 모델 테스트
└── status_tests.rs     # EmailMessageStatus 열거형 테스트
```

### 테스트 격리

```rust
// ✅ Good: 각 테스트는 독립적인 인메모리 DB 사용
#[tokio::test]
async fn test_independent_1() {
    let db = setup_db().await;  // 새로운 인메모리 DB
    // ...
}

#[tokio::test]
async fn test_independent_2() {
    let db = setup_db().await;  // 별도의 인메모리 DB
    // ...
}
```

---

## 🚨 알려진 제한사항

1. **요청당 이메일 수**: 최대 10,000개
2. **Rate Limiting**: `MAX_SEND_PER_SECOND` 환경변수로 제어
3. **DB 크기**: SQLite 단일 파일
4. **동시성**: 스케줄러 단일 인스턴스

---

## 🤝 기여 가이드라인

1. **브랜치 네이밍**: `feature/기능명`, `fix/버그명`
2. **커밋 메시지**: `[모듈명] 변경 내용 요약`
3. **테스트 통과**: `cargo test` 전체 통과
4. **Clippy 통과**: `cargo clippy` 경고 없음
5. **코드 포맷팅**: `cargo fmt` 적용

---

*최종 업데이트: 2025-12-27*
