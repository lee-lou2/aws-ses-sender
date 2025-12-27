# 📧 AWS SES 이메일 발송기

[한국어](README.ko.md) | [English](README.md)

AWS SES와 SNS를 활용한 고성능 대량 이메일 발송 및 모니터링 서버입니다.
Rust와 Tokio를 기반으로 구축되어 높은 처리량과 안정성을 제공합니다.

## 🏗 시스템 아키텍처

### 기술 스택
- 🦀 **Backend**: Rust + Axum
- 📨 **Email Service**: AWS SES
- 🔔 **Notification**: AWS SNS
- 🔄 **Async Runtime**: Tokio
- 💾 **Database**: SQLite
- 🔒 **인증**: X-API-KEY 헤더
- 📊 **모니터링**: Sentry + tracing

### 동작 방식

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

#### 즉시 발송
1. API 요청 수신 (`/v1/messages`)
2. **배치 INSERT**로 DB 저장 → 발송 채널로 전달
3. Token Bucket + Semaphore 기반 Rate Limiting
4. 결과 배치 업데이트 (트랜잭션당 100건)

#### 예약 발송
1. API 요청 수신 (`scheduled_at` 포함)
2. `Created` 상태로 저장
3. 스케줄러가 10초마다 폴링, 원자적으로 메일 픽업 (UPDATE...RETURNING)
4. 즉시 발송과 동일한 흐름으로 처리

## ⚡ 성능 최적화

### Rate Limiting (Token Bucket + Semaphore)
- **Token Bucket**: `Notify` 기반 이벤트 드리븐 방식 (폴링 없음)
- **Semaphore**: 동시 네트워크 요청 제한 (rate limit의 2배)
- **부드러운 리필**: 100ms마다 10%씩 균등 분배
- **논블로킹 채널 전송**: `try_send()`로 즉시 전송

### 데이터베이스 (SQLite + WAL)
- **WAL 모드**: 쓰기 중에도 동시 읽기 가능
- **mmap**: 256MB 메모리 맵 I/O
- **캐시**: 64MB 인메모리 캐시 + temp_store 메모리 사용
- **자동 vacuum**: Incremental vacuum으로 저장소 최적화
- **배치 INSERT**: 멀티-로우 INSERT로 **10배 이상** 성능 향상
- **배치 업데이트**: `CASE WHEN` 문법으로 벌크 업데이트
- **2단계 스케줄러**: UPDATE...RETURNING + JOIN으로 효율적 폴링
- **복합 인덱스**: 스케줄러, 카운트, stop 쿼리 최적화
- **콘텐츠 중복 방지**: Subject/content를 별도 테이블에 저장하여 중복 방지

### 커넥션 풀링
- **SES 클라이언트**: OnceCell로 단일 인스턴스 캐싱
- **DB 풀**: 5-20개 연결, idle timeout 적용
- **채널**: 발송 10,000개, 후처리 1,000개 버퍼

## ✨ 주요 기능

- 🚀 대량 이메일 발송 및 예약 발송
- 📊 실시간 발송 결과 모니터링
- 👀 이메일 열람 추적 (1x1 픽셀)
- ⏸ 대기 중인 이메일 발송 취소
- 📈 토픽별 통계

![img.png](docs/process_diagram_ko.png)

## 🔧 설정 가이드

### AWS SES 설정하기

#### 1️⃣ 샌드박스 모드 해제 (프로덕션 환경)
- [AWS Support Center에서 샌드박스 해제 요청](https://docs.aws.amazon.com/ses/latest/dg/request-production-access.html)

#### 2️⃣ 도메인 인증
- AWS SES 콘솔에서 도메인 등록
- DNS에 DKIM, SPF 레코드 추가

#### 3️⃣ 이메일 주소 인증 (샌드박스 모드)
- AWS SES 콘솔에서 발신자 이메일 등록

### AWS SNS 설정하기 (선택사항)

#### 1️⃣ SNS 주제 생성
- AWS SNS 콘솔에서 새 주제 생성

#### 2️⃣ SES 이벤트 설정
- SNS 이벤트 대상 추가 (Bounce, Complaint, Delivery)

#### 3️⃣ SNS 구독 설정
- 구독 추가 (HTTP/HTTPS 엔드포인트: `/v1/events/results`)

![img_1.png](docs/aws_diagram.png)

## ⚙️ 환경 변수

```env
# AWS 설정
AWS_REGION=ap-northeast-2
AWS_ACCESS_KEY_ID=your_access_key
AWS_SECRET_ACCESS_KEY=your_secret_key
AWS_SES_FROM_EMAIL=your_verified_email

# 서버 설정
SERVER_URL=http://localhost:3000
SERVER_PORT=3000
API_KEY=your_api_key
MAX_SEND_PER_SECOND=24

# 선택사항
SENTRY_DSN=your_sentry_dsn
RUST_LOG=info
```

## 🚀 빠른 시작

```bash
# 데이터베이스 초기화
./init_database.sh

# 서버 실행
cargo run --release

# Docker로 실행
docker build -t ses-sender .
docker run -p 3000:3000 --env-file .env ses-sender
```

## 📡 API 가이드

### 이메일 발송

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
      "subject": "1월 뉴스레터",
      "content": "<h1>안녕하세요!</h1><p>...</p>"
    }
  ],
  "scheduled_at": "2024-01-01 09:00:00"
}
```

**응답:**
```json
{
  "total": 2,
  "success": 2,
  "errors": 0,
  "duration_ms": 45,
  "scheduled": true
}
```

### 이벤트 추적

| 엔드포인트 | 메서드 | 설명 |
|----------|--------|-------------|
| `/v1/events/open?request_id={id}` | GET | 이메일 열람 추적 (1x1 PNG 반환) |
| `/v1/events/counts/sent?hours=24` | GET | 발송 건수 조회 (최근 N시간) |
| `/v1/events/results` | POST | AWS SNS 이벤트 수신 |

### 토픽 관리

| 엔드포인트 | 메서드 | 설명 |
|----------|--------|-------------|
| `/v1/topics/{topic_id}` | GET | 토픽별 통계 조회 |
| `/v1/topics/{topic_id}` | DELETE | 대기 중인 이메일 발송 취소 |

## 🧪 테스트

```bash
# 전체 테스트 실행
cargo test

# 출력과 함께 실행
cargo test -- --nocapture

# 특정 테스트 실행
cargo test test_save_batch
```

## 📊 모니터링

### 로그 레벨
```bash
RUST_LOG=debug cargo run  # 상세 로그
RUST_LOG=info cargo run   # 일반 운영
RUST_LOG=warn cargo run   # 경고만
```

### 헬스 체크
```bash
curl http://localhost:3000/v1/events/counts/sent \
  -H "X-API-KEY: $API_KEY"
```

## 📁 프로젝트 구조

```
src/
├── main.rs                 # 진입점, 초기화
├── app.rs                  # 라우터 설정
├── config.rs               # 환경변수 관리
├── state.rs                # 애플리케이션 상태
├── handlers/               # HTTP 요청 핸들러
│   ├── message_handlers.rs # 이메일 발송 API
│   ├── event_handlers.rs   # SNS 이벤트, 오픈 트래킹
│   └── topic_handlers.rs   # 토픽 관리
├── services/               # 백그라운드 서비스
│   ├── scheduler.rs        # 예약 이메일 조회
│   ├── receiver.rs         # Rate-limited 발송
│   └── sender.rs           # AWS SES API 호출
├── models/                 # 데이터 모델
│   ├── content.rs          # EmailContent (subject, content 저장)
│   ├── request.rs          # EmailRequest, EmailMessageStatus
│   └── result.rs           # EmailResult
├── middlewares/            # HTTP 미들웨어
│   └── auth_middlewares.rs # API Key 인증
└── tests/                  # 단위 및 통합 테스트
    ├── helpers (mod.rs)    # 공유 테스트 유틸리티
    ├── auth_tests.rs
    ├── event_tests.rs
    ├── handler_tests.rs
    ├── request_tests.rs
    ├── scheduler_tests.rs
    ├── status_tests.rs
    └── topic_tests.rs
```

## 🛠 개발 가이드

### 코드 스타일

이 프로젝트는 Rust 공식 스타일 가이드를 따릅니다:

```bash
# 코드 포맷팅
cargo fmt

# 린터 실행
cargo clippy

# 모든 검사 실행
cargo clippy -- -W clippy::all -W clippy::pedantic
```

**Lint 설정 (Cargo.toml):**
```toml
[lints.rust]
unsafe_code = "forbid"

[lints.clippy]
all = "warn"
pedantic = "warn"
nursery = "warn"
```

### 주요 의존성

| 크레이트 | 용도 |
|-------|---------|
| `axum` | 웹 프레임워크 |
| `tokio` | 비동기 런타임 |
| `sqlx` | 데이터베이스 (SQLite) |
| `aws-sdk-sesv2` | AWS SES API |
| `serde` / `serde_json` | 직렬화 |
| `thiserror` | 에러 처리 |
| `tracing` | 로깅 |
| `sentry` | 에러 트래킹 |

### 빌드

```bash
# 개발 빌드
cargo build

# 릴리즈 빌드 (최적화)
cargo build --release

# 빌드 없이 검사만
cargo check
```

## 📚 참고 자료

- [AWS SES 개발자 가이드](https://docs.aws.amazon.com/ses/latest/dg/Welcome.html)
- [AWS SNS 개발자 가이드](https://docs.aws.amazon.com/sns/latest/dg/welcome.html)
- [Axum 문서](https://docs.rs/axum)
- [SQLx 문서](https://docs.rs/sqlx)
- [Rust API 가이드라인](https://rust-lang.github.io/api-guidelines/)

## 📄 라이선스

MIT License
