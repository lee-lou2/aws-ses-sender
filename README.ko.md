# 📧 AWS SES 이메일 발송 서비스

[한국어](README.ko.md) | [English](README.md)

**Rust**와 **AWS SES**로 구축한 고성능 대량 이메일 발송 서비스입니다.

## ✨ 주요 기능

- 🚀 **대량 발송** — 요청당 최대 10,000개 이메일 처리
- ⏰ **예약 발송** — 지정한 시간에 이메일 발송
- ⚡ **속도 제어** — Token Bucket + Semaphore 기반 정밀 제어
- 📊 **이벤트 추적** — AWS SNS를 통한 Bounce, Complaint, Delivery 수신
- 👀 **오픈 추적** — 1x1 투명 픽셀로 열람 감지
- ⏸️ **발송 취소** — 토픽별 대기 중인 이메일 취소

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

### 동작 방식

**즉시 발송:**
1. API 요청 수신 → DB에 배치 INSERT
2. 발송 채널로 전달 → 속도 제어하며 발송
3. 결과 배치 업데이트 (트랜잭션당 100건)

**예약 발송:**
1. `scheduled_at` 포함된 API 요청 수신
2. `Created` 상태로 저장
3. 스케줄러가 10초마다 폴링 → 발송 시간 도래한 이메일 픽업
4. 즉시 발송과 동일한 흐름으로 처리

---

## ⚡ 성능 최적화

| 최적화 항목 | 설명 |
|-------------|------|
| **Token Bucket** | Atomic CAS 기반 정밀한 초당 속도 제어 |
| **Semaphore** | 동시 네트워크 요청 제한 (속도 제한의 2배) |
| **WAL 모드** | SQLite 쓰기 중 동시 읽기 지원 |
| **배치 INSERT** | 멀티-로우 INSERT로 10배 성능 향상 |
| **배치 업데이트** | 트랜잭션당 100건 처리 |
| **커넥션 풀** | 5-20개 DB 연결, idle timeout 적용 |

---

## 🚀 시작하기

### 사전 요구사항

- Rust 1.70 이상
- AWS 계정 (SES 설정 완료)
- (선택) 이벤트 알림용 AWS SNS

### 1. 프로젝트 클론 및 설정

```bash
git clone https://github.com/your-repo/aws-ses-sender.git
cd aws-ses-sender

# 데이터베이스 초기화
./init_database.sh

# .env 파일 생성
cp .env.example .env
```

### 2. 환경 변수 설정

```env
# 필수
SERVER_URL=https://your-domain.com
API_KEY=your-secure-api-key
AWS_SES_FROM_EMAIL=noreply@your-domain.com

# 선택
SERVER_PORT=8080
AWS_REGION=ap-northeast-2
MAX_SEND_PER_SECOND=24
SENTRY_DSN=your-sentry-dsn
RUST_LOG=info
```

### 3. 실행

```bash
# 개발 모드
cargo run

# 프로덕션 모드
cargo run --release

# Docker
docker build -t ses-sender .
docker run -p 8080:8080 --env-file .env ses-sender
```

---

## 📡 API 가이드

### 인증

보호된 엔드포인트는 `X-API-KEY` 헤더가 필요합니다:

```http
X-API-KEY: your-api-key
```

### 엔드포인트 목록

| 메서드 | 엔드포인트 | 인증 | 설명 |
|--------|-----------|------|------|
| POST | `/v1/messages` | ✅ | 이메일 발송 |
| GET | `/v1/topics/{id}` | ✅ | 토픽 통계 조회 |
| DELETE | `/v1/topics/{id}` | ✅ | 대기 중인 이메일 취소 |
| GET | `/v1/events/open` | ❌ | 이메일 열람 추적 |
| GET | `/v1/events/counts/sent` | ✅ | 발송 건수 조회 |
| POST | `/v1/events/results` | ❌ | AWS SNS 웹훅 |

### 이메일 발송

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
      "subject": "1월 뉴스레터",
      "content": "<h1>안녕하세요!</h1><p>뉴스레터에 오신 것을 환영합니다.</p>"
    }
  ],
  "scheduled_at": "2024-01-15 09:00:00"
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

### 토픽 통계 조회

```http
GET /v1/topics/newsletter_2024_01
X-API-KEY: your-api-key
```

**응답:**

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

### 대기 중인 이메일 취소

```http
DELETE /v1/topics/newsletter_2024_01
X-API-KEY: your-api-key
```

`Created` 상태(아직 발송되지 않은)의 이메일만 취소됩니다.

### 발송 건수 조회

```http
GET /v1/events/counts/sent?hours=24
X-API-KEY: your-api-key
```

**응답:**

```json
{
  "count": 1523
}
```

---

## 🔧 AWS 설정

### SES 설정

1. **도메인 인증**
   - AWS SES 콘솔 → 확인된 자격 증명
   - 도메인 추가 후 DKIM/SPF 레코드 설정

2. **샌드박스 해제** (프로덕션용)
   - [AWS Support](https://docs.aws.amazon.com/ses/latest/dg/request-production-access.html)를 통해 프로덕션 액세스 요청

3. **IAM 권한**
   ```json
   {
     "Effect": "Allow",
     "Action": ["ses:SendEmail", "ses:SendRawEmail"],
     "Resource": "*"
   }
   ```

### SNS 설정 (선택사항)

이벤트 추적(Bounce, Complaint, Delivery)을 위한 설정:

1. **SNS 주제 생성**
   - AWS SNS 콘솔 → 주제 생성

2. **SES 이벤트 설정**
   - SES 콘솔 → 구성 세트 → 이벤트 대상
   - Bounce, Complaint, Delivery에 SNS 대상 추가

3. **엔드포인트 구독**
   - HTTP/HTTPS 구독 추가: `https://your-domain.com/v1/events/results`
   - 구독 확인 (API가 자동 처리)

![AWS 아키텍처](docs/aws_diagram.png)

---

## 📊 모니터링

### 로그 레벨

```bash
RUST_LOG=debug cargo run  # 상세 출력
RUST_LOG=info cargo run   # 일반 운영
RUST_LOG=warn cargo run   # 경고만 출력
```

### 헬스 체크

```bash
curl -H "X-API-KEY: $API_KEY" \
  http://localhost:8080/v1/events/counts/sent
```

### Sentry 연동

`SENTRY_DSN` 환경 변수를 설정하면 에러 추적이 활성화됩니다.

---

## 🧪 테스트

```bash
# 전체 테스트 실행
cargo test

# 출력과 함께 실행
cargo test -- --nocapture

# 특정 테스트 실행
cargo test test_save_batch

# 특정 모듈 테스트 실행
cargo test request_tests
```

---

## 📁 프로젝트 구조

```
src/
├── main.rs           # 진입점, 초기화
├── app.rs            # 라우터 설정
├── config.rs         # 환경 변수
├── state.rs          # 애플리케이션 상태
├── handlers/         # HTTP 핸들러
│   ├── message_handlers.rs
│   ├── event_handlers.rs
│   └── topic_handlers.rs
├── services/         # 백그라운드 서비스
│   ├── scheduler.rs  # 예약 이메일 조회
│   ├── receiver.rs   # 속도 제어 발송
│   └── sender.rs     # AWS SES 클라이언트
├── models/           # 데이터 모델
│   ├── request.rs    # EmailRequest
│   └── result.rs     # EmailResult
├── middlewares/      # HTTP 미들웨어
│   └── auth_middlewares.rs
└── tests/            # 테스트 모듈
```

---

## 🛠 개발

### 코드 품질

```bash
# 코드 포맷팅
cargo fmt

# 린터 실행
cargo clippy

# 릴리즈 빌드
cargo build --release
```

### 주요 의존성

| 크레이트 | 용도 |
|---------|------|
| `axum` | 웹 프레임워크 |
| `tokio` | 비동기 런타임 |
| `sqlx` | 데이터베이스 (SQLite) |
| `aws-sdk-sesv2` | AWS SES 클라이언트 |
| `serde` | 직렬화 |
| `thiserror` | 에러 처리 |
| `tracing` | 로깅 |
| `sentry` | 에러 추적 |

---

## 📄 라이선스

MIT License

---

## 📚 참고 자료

- [AWS SES 개발자 가이드](https://docs.aws.amazon.com/ses/latest/dg/Welcome.html)
- [AWS SNS 개발자 가이드](https://docs.aws.amazon.com/sns/latest/dg/welcome.html)
- [Axum 문서](https://docs.rs/axum)
- [SQLx 문서](https://docs.rs/sqlx)
