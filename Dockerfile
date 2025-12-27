# =============================================================================
# Build Stage - Rust 빌드
# =============================================================================
FROM rust:1.83-slim-bookworm AS builder

# 빌드에 필요한 최소 의존성만 설치
RUN apt-get update && apt-get install -y --no-install-recommends \
    pkg-config \
    libssl-dev \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /app

# 의존성 캐싱을 위해 Cargo 파일만 먼저 복사
COPY Cargo.toml Cargo.lock ./

# 더미 소스로 의존성만 빌드 (캐싱)
RUN mkdir src && \
    echo "fn main() {}" > src/main.rs && \
    cargo build --release && \
    rm -rf src target/release/deps/rust_aws_ses_sender*

# 실제 소스 복사 및 빌드
COPY src ./src
RUN cargo build --release

# =============================================================================
# Runtime Stage - 최소 런타임 환경
# =============================================================================
FROM debian:bookworm-slim

# 런타임에 필요한 최소 의존성만 설치
RUN apt-get update && apt-get install -y --no-install-recommends \
    libssl3 \
    ca-certificates \
    && rm -rf /var/lib/apt/lists/* \
    && rm -rf /var/cache/apt/*

# 비루트 사용자 생성 (보안)
RUN useradd -r -s /bin/false appuser

WORKDIR /app

# 바이너리 복사
COPY --from=builder /app/target/release/rust-aws-ses-sender ./

# 소유권 설정
RUN chown -R appuser:appuser /app

# 비루트 사용자로 전환
USER appuser

# 포트 노출
EXPOSE 3000

# 실행
CMD ["./rust-aws-ses-sender"]
